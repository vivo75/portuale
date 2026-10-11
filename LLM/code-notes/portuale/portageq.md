# crates/portuale/src/portageq.rs
Native `has_version` / `best_version` over the installed-package database.

## module
- Feature feat#157, backlog #305 S6.2. Portage's `bin/portageq` is a Python program that reads `var/db/pkg` directly, so on a database-backed ROOT it cannot see the installed packages; this applet answers the same two questions from the database behind `portage_vdb`.
- Behaviour contract: `LLM/evidence/305-s6-portageq.md` ("Rules the native implementation must follow", rules 1-10), taken from Portage's `bin/portageq:80-203` and `:1408-1425`. Design: `docs/vdb_to_db.md` §10.
- Matching reuses the resolver's own installed-package matcher (`portage_repo::best_installed_match`, the `best_installed_matching` core); there is no second matcher here. Matching details: slot, sub-slot, version operators, `::repo`, USE deps against the installed `USE`.
- Backend wiring: `PORTUALE_VDB_ROOT` (also exported by `mrg`) names the ROOT the database belongs to; the database is registered only for that root (`has_version -b` asks about BROOT `/` while ROOT is a chroot, and that other root keeps the files layout). Without `PORTUALE_VDB_ROOT` the database is used for whatever <eroot> is given (a hand-set environment names the backend explicitly).
- redb (S6.3, owner question Q5): Portage's `ebuild-ipc` pattern (`vdb_ipc.rs`). When the backend is redb, the <eroot> is the database's root and `PORTUALE_VDB_IPC` is set, every installed-package lookup goes to the parent: the match (`best_installed_match`) and `dep_expand`'s category lookup. Everything else -- rules 1-10, the strict parse, the EAPI QA notice, USE-conditional evaluation, the implicit IUSE from the config, the output and exit codes -- stays here, in one place for every backend. A pipe that cannot be reached, or an `err` reply, is exit 4 (never 1: that would read as "not installed"). The parent answers from its last committed state: during `pkg_preinst` the old instance, until the new one is published. Files and sqlite never use the pipe (they open the database directly, read-only); redb without the variable keeps S5.4's behaviour: read directly when nobody holds the file, exit 4 naming the missing variable when somebody does.

## const EX_INTERNAL
- `phase-helpers.sh` reports it as `unexpected portageq exit code`.

## fn run_query
- Rule 1 (insufficient parameters), rule 3 (strictness), rule 4 (strict invalid atom), rule 5 (QA notice), rule 2 (USE-conditional evaluation), rule 6 (`dep_expand` fall-through), rule 7 (matcher), rule 8 (`has_version` exit), rule 9 (`best_version` output) -- see the evidence doc.
- The <eroot>-is-a-directory check is Portage's `uses_eroot` (portageq:1469-1475).

## fn setup_backend
- Backlog #318: follows make.conf's `PORTUALE_VDB_BACKEND` / `PORTUALE_VDB_PATH`, as the applets do, so every tool reads the database `emerge` writes; phases never pay for config resolution when a value is exported.

## fn eapi_number
- Portage `_get_eapi_attrs` (eapi.py:48) returns the permissive set for a missing/unsupported EAPI.

## fn eapi_has_repo_deps
- False for every numbered EAPI, true for the permissive set.

## fn eapi_violation
- EAPI-aware atom validation (Portage `Atom(..., eapi=)`): ports the error Portage's `Atom(raw, allow_repo=..., eapi=eapi)` raises (dep/__init__.py:2090-2150). Pre-EAPI-5 slot grammar rejects the atom outright in Portage (`InvalidAtom(self._string)`).

## fn eqawarn
- QA notices: Portage `elog("eqawarn", ...)` (`elog("eqawarn", lines)`, portageq:1408-1425). Portage's portageq runs `bash -c "source '$PORTAGE_BIN_PATH/isolated-functions.sh' ; eqawarn '<line>' ; ..."`, so the notice goes through the ebuild's own elog machinery (log file, `PORTAGE_ELOG_*`). Do the same when `PORTAGE_BIN_PATH` names a directory holding `isolated-functions.sh`. Otherwise (no phase environment) print ` * <line>` on stderr. The plain-text ` * <line>` on stderr is the plain-text form of `eqawarn`'s output.

## fn dep_expand
- Ports `portage/dbapi/dep_expand.py` (the fall-through path) and `cpv_expand`: what `vardb.match()` does to a string the strict `Atom()` parse rejected: a missing `=` prefix is allowed (`cat/pkg-1.0`), a bare name expands to the one installed category, none installed leaves `null/<pn>` which matches nothing.
- Rule 6's divergence: Portage's `best_version` dies with a traceback on an invalid atom; same exit code here with the ERROR line instead.
- `AmbiguousPackageName` is uncaught under Portage too (exit 1).

## tests
- Each test's expected value comes from Portage, per the evidence captures in `LLM/evidence/305-s6-portageq.md`: `found_and_not_found` (captures 1, 2), `versioned_atoms` (3), `slot_atoms` (4), `repo_atoms` (14), `use_deps_against_installed_use` (5, 6), `use_conditionals_follow_the_environment_use` (7, 8), `strict_invalid_atom_exits_2` (10), `best_version_prints_the_highest_match` (11, 13), `best_version_no_match_prints_an_empty_line` (12), `insufficient_parameters` (15), `non_strict_bare_names_expand` (9). The rule number in each docstring is the pin to the contract's rules 1-10.
- The two sqlite/files backends must agree (S6.2 acceptance).
- `use_deps_against_installed_use`: Portage `_match_use`; the dropped conditional's flag must still be installed `IUSE` because Portage checks the unevaluated atom's `.required`.
- `strict_eapi_only_invalid_warns_and_still_matches` (rules 3 and 5): strict, the atom parses without an EAPI but is invalid for `EAPI`: a QA notice, and the atom still matches. With no `PORTAGE_BIN_PATH` the notice is ` * QA Notice: ...` on stderr.
- `non_strict_bare_names_expand`: Portage `dep_expand` / `cpv_expand`.
- `unknown_command_and_bad_eroot`: Portage's `Not a directory` usage error.
- `matches_portageq_on_the_fixture_root`: compares against Portage's own `bin/portageq` from the `3rdparty/portage` checkout.
