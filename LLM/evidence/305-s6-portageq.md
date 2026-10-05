# Evidence: portageq native implementation contract

## Sources

**portageq** (Python CLI):
- `/home/vivo/repo/PORTUALE/portuale/3rdparty/portage/bin/portageq`
  - `has_version()` function: lines 121–164
  - `best_version()` function: lines 166–203
  - Global setup (atom validation, EAPI): lines 1408–1425

**phase-helpers.sh** (Bash function wrapper):
- `/home/vivo/repo/PORTUALE/portuale/3rdparty/portage/bin/phase-helpers.sh`
  - `___best_version_and_has_version_common()` helper: lines 852–935
  - `has_version()` wrapper: lines 937–945
  - `best_version()` wrapper: lines 947–955

**portageq-wrapper** (exec shim):
- `/home/vivo/repo/PORTUALE/portuale/3rdparty/portage/bin/portageq-wrapper`
  - Execution chain: lines 1–19

**portuale Rust integration**:
- `/home/vivo/repo/PORTUALE/portuale/rust/portuale/src/ebuild_phases.rs`
  - `portage_checkout()` function: lines 669–674 (reads `PORTUALE_PORTAGE_CHECKOUT` or defaults to `3rdparty/portage`)
  - `bin_dir()` function: lines 697–742 (returns `PORTAGE_BIN_PATH` as symlink overlay or vendored `bin/`)
  - Phase execution setup: lines 3006–3007 (sets `PORTAGE_BIN_PATH` env var)

## Behaviour

**1. has_version and best_version both take an eroot (ROOT) and an atom as arguments.**
  - Source: portageq lines 122–123, 167–169

**2. has_version returns:**
  - Exit code 0 if the atom matches at least one installed package
  - Exit code 1 if no installed package matches
  - Exit code 2 if the atom is invalid
  - Exit code 3 if insufficient parameters (fewer than 2 args after command name)
  - Source: portageq lines 122–159; test captures

**3. best_version returns:**
  - Exit code 0 in all cases (found or not found)
  - Exit code 2 if the atom is invalid
  - Exit code 3 if insufficient parameters
  - Prints the highest-version matching package (without .ebuild suffix) to stdout when found
  - Prints an empty line (nothing) to stdout when no match is found
  - Source: portageq lines 167–198; test captures (test 12 shows empty line + rc 0)

**4. Atom validation is strict only when `EBUILD_PHASE` is set** (`atom_validate_strict`, portageq setup).
  - The atom is first parsed WITHOUT an EAPI (`Atom(argv[1], allow_repo=…)`, portageq:128-136 / :170-178).
  - Strict, and that parse fails: `ERROR: Invalid atom: '<atom>'` on **stderr**, exit **2**. No eqawarn.
  - Strict, the parse succeeds but a second parse WITH `eapi=$EAPI` fails: the atom is still used; a `QA Notice: has_version: <error>` (or `best_version:`) is sent through `eqawarn` (portageq:138-146 / :180-188).
  - Not strict, and the parse fails: the raw string goes to `vardb.match()`, which runs `dep_expand` + `Atom()` again (vartree.py:851, dep_expand.py:37). A bare name such as `garbage-atom` is expanded as a package name and simply does not match (`has_version` exit 1). A malformed atom (`sys-libs/glibc[`, `=sys-libs/glibc`) raises `InvalidAtom`: `has_version` catches it, prints `ERROR: Invalid atom: '<atom>'` and exits **2** (portageq:156-158); `best_version` does not catch it and dies with a Python traceback, exit **1** (verified 2026-10-05 by the coordinator).

**5. EAPI-dependent atom parsing:**
  - When EBUILD_PHASE is set, EAPI is read from environment to determine parsing rules
  - Repository atoms (::repo) are allowed only if eapi_has_repo_deps(EAPI) returns true or atom_validate_strict is False
  - Source: portageq lines 1410–1411, 129, 174

**6. USE-conditional evaluation:**
  - If USE is set in environment, conditionals like `[sqlite?]` are evaluated against that USE value
  - Example: `dev-lang/python[sqlite?]` with `USE=sqlite` evaluates to `dev-lang/python[sqlite]`
  - Example: `dev-lang/python[sqlite?]` with `USE=""` evaluates to `dev-lang/python`
  - Source: portageq lines 80–84, 144, 189

**7. Atoms with USE dependencies:**
  - `[multilib]` (positive): requires the use flag
  - `[-multilib]` (negative): requires the use flag NOT be set
  - Evaluation depends on the installed package's actual USE flags
  - Source: portageq uses portage.db[eroot]["vartree"].dbapi.match(atom); test captures show results

**8. Repository atoms (::repo):**
  - Syntax: `category/package::repository`
  - Allowed without EBUILD_PHASE set (non-strict mode)
  - Returns 0 (found) if a package in that repo is installed
  - Source: test capture 14; portageq line 129

**9. Eroot argument validation:**
  - Eroot must be a directory and is normalized via portage.util.normalize_path
  - Eroot is used to access portage.db[eroot]["vartree"].dbapi for package queries
  - When eprefix is set, eroot must end with that eprefix
  - Source: portageq lines 1485–1496

**10. Insufficient parameters:**
  - has_version / best_version require at least 2 args (eroot + atom)
  - Fewer args return exit code 3 with "ERROR: insufficient parameters!" on stdout
  - Source: portageq lines 123–125, 168–170; test capture 15

## phase-helpers.sh contract

The shell wrappers in phase-helpers.sh translate flag arguments to root paths and exit code semantics:

**Flag → root mapping** (lines 868–909):

| Flag | EAPI with PREFIX | EAPI without PREFIX | Notes |
|------|------------------|---------------------|-------|
| (none, default) | ROOT + EPREFIX | ROOT | Uses actual ROOT environment variable |
| `--host-root` | /PORTAGE_OVERRIDE_EPREFIX | / | Requires EAPI support (or die) |
| `-r` (ROOT) | ROOT + EPREFIX | ROOT | Requires EAPI support; same as default |
| `-d` (destination/SYSROOT) | ESYSROOT or / | SYSROOT or / | Requires EAPI support |
| `-b` (build/BROOT) | /PORTAGE_OVERRIDE_EPREFIX | / | Requires EAPI support; sets EPREFIX env var |

Source: phase-helpers.sh lines 859–909

**Exit code handling** (lines 920–934):

- RC 0 or 1: passed through to caller (0 = found, 1 = not found)
- RC 2: die with "invalid atom: ${atom}"
- RC 3: (no special handling, but portageq only returns 3 for "insufficient parameters"; phase-helpers expects 0–2)
- RC 4+: die with "unexpected portageq exit code: ${retval}"

Source: phase-helpers.sh lines 920–934

**IPC daemon fallback** (lines 911–917):

If `PORTAGE_IPC_DAEMON` is set in environment, call `${PORTAGE_BIN_PATH}/ebuild-ipc` instead of `portageq-wrapper`.
Otherwise, call `${PORTAGE_BIN_PATH}/portageq-wrapper`.

Source: phase-helpers.sh lines 911–917

## Current portuale wiring

**Environment setup:**

1. `bin_dir()` in `ebuild_phases.rs` (line 697) returns the value of `PORTAGE_BIN_PATH`:
   - A symlink overlay to `3rdparty/portage/bin/` when `portage_checkout()` exists (line 711)
   - Or the vendored `bin/` directory directly (line 713)

2. `portage_checkout()` (line 669) reads `PORTUALE_PORTAGE_CHECKOUT` or defaults to `3rdparty/portage`

3. Phase execution sets `PORTAGE_BIN_PATH` to `bin_dir()` (line 3007) for all ebuild phases

4. `phase-helpers.sh` in the checkout `bin/` directory is sourced, which defines `has_version` and `best_version`

5. When an ebuild calls `has_version` or `best_version`, the shell function calls `${PORTAGE_BIN_PATH}/portageq-wrapper` (or `ebuild-ipc`)

6. `portageq-wrapper` finds `portageq` in `${PORTAGE_BIN_PATH}` and execs it with `PYTHONPATH=${PORTAGE_PYTHONPATH:-${PORTAGE_PYM_PATH}}`

**No IPC daemon:** portuale currently does not set `PORTAGE_IPC_DAEMON`, so the shell functions always use `portageq-wrapper` → `portageq`.

## Captures

Real portageq behavior (host: `/usr/bin/portageq` version 3.0.82.2, Gentoo system):

| # | Command | Env | RC | Stdout | Stderr |
|---|---------|-----|----|---------|----|
| 1 | `has_version / sys-libs/glibc` | none | 0 | (none) | (none) |
| 2 | `has_version / nonexistent/package` | none | 1 | (none) | (none) |
| 3 | `has_version / '>=sys-libs/glibc-2.0'` | none | 0 | (none) | (none) |
| 4 | `has_version / 'sys-libs/glibc:2.2'` | none | 0 | (none) | (none) |
| 5 | `has_version / 'sys-libs/glibc[multilib]'` | `USE=multilib` | 0 | (none) | (none) |
| 6 | `has_version / 'sys-libs/glibc[-multilib]'` | `USE=multilib` | 1 | (none) | (none) |
| 7 | `has_version / 'dev-lang/python[sqlite?]'` | `USE=sqlite` | 0 | (none) | (none) |
| 8 | `has_version / 'dev-lang/python[sqlite?]'` | `USE=` | 0 | (none) | (none) |
| 9 | `has_version / 'garbage-atom'` | none | 1 | (none) | (none) |
| 10 | `has_version / 'garbage-atom'` | `EBUILD_PHASE=setup EAPI=8 PORTAGE_BIN_PATH=/usr/lib/portage/bin` | 2 | (none) | `ERROR: Invalid atom: 'garbage-atom'` |
| 11 | `best_version / sys-libs/glibc` | none | 0 | `sys-libs/glibc-2.44-r3` | (none) |
| 12 | `best_version / nonexistent/package` | none | 0 | (empty line) | (none) |
| 13 | `best_version / dev-lang/python` | none | 0 | `dev-lang/python-3.14.8` | (none) |
| 14 | `has_version / 'sys-libs/glibc::gentoo'` | none | 0 | (none) | (none) |
| 15 | `has_version /` | none | 3 | `ERROR: insufficient parameters!` | (none) |

## Rules the native implementation must follow

(Rewritten by the coordinator from portageq:80-203 and the captures; the
first draft had the invalid-atom rules wrong.)

1. Arguments: `<eroot> <atom>`. Fewer than two: print `ERROR: insufficient parameters!` on **stdout**, exit 3.
2. USE: only when `USE` is present in the environment, evaluate the atom's USE conditionals against `USE.split()` (portageq:80-84). If `USE` is absent, conditionals are left as they are.
3. Strict mode iff `EBUILD_PHASE` is set; then `EAPI` decides the second, EAPI-aware parse and whether `::repo` is allowed (`allow_repo = not strict or eapi_has_repo_deps(EAPI)`).
4. Strict + unparsable atom: stderr `ERROR: Invalid atom: '<atom>'`, exit 2.
5. Strict + valid atom that is invalid for `EAPI`: emit `QA Notice: <has_version|best_version>: <error>` through eqawarn, then match normally.
6. Not strict + unparsable atom: a bare name matches as a package name (normally exit 1); anything else: `has_version` prints `ERROR: Invalid atom: '<atom>'` and exits 2; `best_version` — real crashes with a traceback (exit 1). The native helper should exit 1 there too and print the same `ERROR: Invalid atom` line on stderr instead of a traceback (documented divergence: no traceback).
7. Match against the installed packages of `<eroot>` (`vardb.match`): slot, sub-slot, version operators, `::repo`, USE deps checked against the installed package's USE.
8. `has_version`: exit 0 if any match, 1 if none; prints nothing. An unknown `<eroot>` (KeyError) is exit 1.
9. `best_version`: print the best match (`portage.best`) followed by a newline, or an empty line when there is none; exit 0. Unknown `<eroot>`: exit 1.
10. phase-helpers.sh treats any exit code other than 0/1 from has_version as fatal (`unexpected portageq exit code`), and maps `-b`/`-d`/`-r` to BROOT / ESYSROOT / EROOT as listed above.
