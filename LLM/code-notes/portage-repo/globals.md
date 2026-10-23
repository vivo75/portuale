# crates/portage-repo/src/globals.rs
Process-wide switches the CLI sets once per run.

## static PACKAGE_MOVES_ENABLED
- Every configured repo's `profiles/updates/` directives, concatenated in `find_repos` (`portdbapi.getRepositories`) order, parsed once and memoised. Reads `$PORTAGE_CONFIGROOT` directly -- the same process-global, env-driven pattern `portuale::color`'s `color.map` override table uses, so no threading through the resolver's ~35-arg signature. An unreadable `repos.conf` yields no updates.
- `--package-moves` (Portage's `y_or_n`, `main.py:936`, default `y`): whether `profiles/updates/` `move`/`slotmove` directives are applied at all. Portage gates the disk-rewriting `_global_updates` pass on `--package-moves != "n"` (`actions.py:3675`); portuale applies moves only at read time, so `=n` turns every `apply_updates_to_*` call into a no-op (the resolver then sees the pre-move `cat/pkg` names throughout). Process-global (env-free, like `color`'s override map) so it needn't thread through the resolver signature; the CLI layer sets it once before resolution.

## fn set_package_moves_enabled
- Default (never called) is `true`.

## static USEOLDPKG_ATOMS
- `--useoldpkg-atoms ATOMS` (Portage's `main.py:713`, `action: "append"` -> `depgraph.py:370-371` `WildcardPackageSet(atoms)`). For a matching package, Portage's `_select_pkg` collects the *built* (binary) candidates into `matched_oldpkg` (`depgraph.py:7936`/`8337`) and, when non-empty, restricts `visible_matches` -- and therefore the best-version pick -- to them: "prefer matching binary packages over newer unbuilt packages" (`--help`). Only reachable when `--usepkg`/`--getbinpkg` puts binary candidates in the pool at all.
- Process-global set once by the CLI layer (same env-free pattern as `--package-moves` / `--color`'s override map) so it needn't thread through `resolve_pretend`'s / `resolve_pretend_graph`'s huge signatures. Same two-tier `matches_config_entry` wildcard matcher `--reinstall-atoms` / `--exclude` already use.

## fn set_useoldpkg_atoms
- Default (never called) is empty -- a strict no-op.

## fn useoldpkg_atom_matches
- Mirrors `useoldpkg_atoms.findAtomForPackage`.

## static BINPKG_CHANGED_DEPS_OVERRIDE
- Explicit `--binpkg-changed-deps=y|n` override (`create_depgraph_params.py:196-203`). `None` = not given, so the auto default applies (`binpkg_changed_deps` on unless `--usepkgonly`); `Some(true)` = `=y` (force on, even under `--usepkgonly`); `Some(false)` = `=n` (force off, even for an ordinary `--getbinpkg`). Process-global, same "CLI sets it once before resolution" pattern.

## fn set_binpkg_changed_deps_override
- Default (never called) is `None` -- the auto behavior.

## fn binpkg_changed_deps_active
- Mirrors Portage's auto default (`myparams["binpkg_changed_deps"] = "auto"` iff `--usepkgonly` not given -> `depgraph.py:7828`'s `!= "n"` -> `True`).

## static USE_EBUILD_VISIBILITY
- `--use-ebuild-visibility[=y|n]` (Portage's `main.py:1103`, `depgraph.py`'s `use_ebuild_visibility = myopts.get(...) != "n"`): when set, `_equiv_ebuild_visible` is enforced on a built candidate even under `--usepkgonly` / a `--useoldpkg-atoms` match (both otherwise exempt it). Portage default is off. Process-global, same pattern.

## fn set_use_ebuild_visibility
- Default (never called) is `false`.

## static RESOLVER_DEBUG
- `--debug` / `-d` (Portage's `main.py:1235`): besides `PORTAGE_DEBUG=1` in the ebuild phase env (`set -x`), Portage calls `initialize_logger(logging.DEBUG)`, turning on `depgraph.py`'s `writemsg_level(..., level=logging.DEBUG)` resolver trace -- the `Arg:`/`Atom:` / `Parent:`/`Depstring:`/`Priority:`/`Candidates:` / `Child:`/`Parent Dep:`/`Exiting...` per-package narration on stdout, and the `\ndigraph:\n\n` + `debug_print()` graph dump plus the per-atom `ebuild:`/`binary:`/`installed:` candidate list on stderr. Portuale emits the same shapes (see `resolver_trace`), gated on this flag.
- Process-global, env-free (same pattern as `USE_EBUILD_VISIBILITY`) so it needn't thread through `resolve_pretend_graph`'s ~60-arg signature. The CLI layer (`pretend.rs`) sets it once, on the `--pretend` path only, before any resolution. Default (never called) is `false`.

## static UNPARSED_DEP_TOKENS
- Portage's `use_reduce(token_class=Atom)` raises `InvalidDependString` on an unparseable token; portuale skips it, so a grammar gap in `portage-dep` would otherwise vanish silently from the graph (`docs/history/second_python_copy_removal.md` §4). `emerge --pretend` reports the total on stderr when `PORTUALE_REPORT_UNPARSED_DEP_TOKENS` is set.

## fn parse_dep_atom
- Backlog #492 (plan `LLM/02.492-dep-drop-ledger.opus.md`). Single choke for parsing a dependency token: `None` counts the token against its call site (`UNPARSED_DEP_SITES`, read with `unparsed_dep_sites`), so a skipped dependency is visible in the `PORTUALE_REPORT_UNPARSED_DEP_TOKENS` report instead of vanishing. Grammar tokens (`||`, `^^`, `??`, parentheses, `flag?`) read as `None` uncounted because raw vdb `*DEPEND` strings are fed in whole (found by the pmtest gate, 2026-10-23). Sites that intend to skip keep `portage_dep::parse_atom` with a `// drop-ok: <reason>` comment; `tests/dep_drop_lint.rs` scans `resolve/`, `slot_operator/`, `merge_order/` and `solver_bridge.rs` for any other bare call.

## static EBUILD_DB_DROPPED
- Backlog #396. `--usepkgonly=n` stores `False` under the key, and `"--usepkgonly" not in myopts` (`depgraph.py:809`) is then false, so Portage builds its dbs list without the ebuild database although binaries are not exclusive. Probe: `-pv --usepkgonly=n <new package>` is rc 1 `no ebuilds to satisfy`, not a merge. A process-global rather than a `ResolveRequest` field because the pool is built at four sites (`candidate_pool`, `greedy_slots`, `resolve_pretend`, the dependency installed fallback) and `SelectOptions` has 35 construction sites. Evidence: `LLM/evidence/2026-10-18-s8/out/c396-*.txt`.

## static DISPLAY_ROOT_SUFFIX
- Backlog #395. `_show_unsatisfied_dep` appends ` for <root>` to the quoted atom when `root != _running_root.root` (`depgraph.py:6502-6503`). Portage's running root is the real `/`; portuale takes it from `PORTAGE_RUNNING_ROOT` (owner Q3: the hook stays), default `/`. The suffix has the root's trailing slash, and the five messages built from `xinfo` carry it: `no ebuilds`, `no binary packages`, `no ebuilds built with USE flags`, `All ebuilds … have been masked` and `The ebuild selected to satisfy … has unmet requirements`.
