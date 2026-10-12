# crates/portuale/src/pretend/cli_atoms.rs
Command-line atom validation: package arguments and the --exclude-style option families (backlog #341 S7).

## fn insert_category_into_atom
- Portage `_emerge/is_valid_package_atom.py:19-27`: splice `category/` before the first `[\*\w]` (ASCII) character, `None` when the string holds none. Both `is_valid_package_atom` (`"cat"`) and `_find_bad_atoms` (`"dummy-category"`, `repository/config.py:91-116`) share it; an operator/blocker prefix stays outside the splice (`=t341-dep-1.0` -> `=cat/t341-dep-1.0`).

## fn is_valid_package_atom
- Portage `_emerge/is_valid_package_atom.py:is_valid_package_atom(x, allow_repo=True)`: short names get `cat/`, then the atom must parse with `allow_repo=True` (build-ids on via the `eapi=None` default, no wildcards) and carry no blocker. This is the `run_action` gate (`_emerge/actions.py:4244-4262`, two `!!!` lines, exit 1). The `depgraph._select_files` three-line variant (depgraph.py:5261-5269, with the `(Did you specify a version …)` line) is unreachable behind it and is not ported; `action_config` (`emerge --config`, actions.py:717-724) stays as `config_action.rs` renders it.
- Existing filesystem paths skip the check (Portage's `os.sep`-prefix / `os.lstat` skips in the same loop); the merge path otherwise treats everything as atoms, as before.

## fn find_bad_atoms
- Portage `portage/repository/config.py:91-116` (`_find_bad_atoms`): bad entries in order, the value re-split on whitespace exactly like `" ".join(atoms).split()`. Strict mode (`allow_repo=False`) rejects any operator, blocker, use dep, repo or build-id form (build-ids only parse on `=`, so the operator rule covers them); `less_strict` (only `--buildpkg-exclude`, `_emerge/main.py:839-845`, where `_find_bad_atoms` passes no `allow_build_id` so `eapi=None` turns it on) accepts operators, repos and build-ids. Only parse failures, blockers and use deps stay bad there.

## fn validate_exclude_options
- Portage `_emerge/main.py:839-845,885-906` (option post-pass): `--buildpkg-exclude` first with its own message (no "(only …)" suffix), then the nine strict families (`exclude`, `getbinpkg-exclude`, `getbinpkg-include`, `reinstall-atoms`, `rebuild-exclude`, `rebuild-ignore`, `usepkg-exclude`, `usepkg-include`, `useoldpkg-atoms`) in tuple order, skipping empty ones (the generator's `if atoms`). Only Portage's `parser.error` line is reproduced; the ~4.7 KB argparse `usage:` block above it is not (S0 matrix §1.5). Exit 2. Runs at the top of `validate_and_early_actions`, so it covers every action path like Portage's global post-pass.
