# crates/portage-repo/src/repo_config.rs
repos.conf discovery and per-repository configuration.

## struct RepoConfig::is_main
- Scopes the main repo's own `package.mask`/`.unmask`/`package.use` family with `::reponame` the same way every overlay's entries already are; an overlay's repo-level config is fully in scope now (see portage-profile's module doc for the overlay-scoping follow-ups that closed this out).
- Whether this is `repos.conf`'s `[DEFAULT] main-repo` -- needed by callers that read repo-level config keyed by repo identity (e.g. `portage_profile::resolve_config`'s own `main_repo_name` param, which scopes the main repo's own `package.mask`/`.unmask`/`package.use` family with `::reponame` the same way every overlay's own entries already are).

## struct RepoConfig::masters
- Ported in `find_repos` (`RepoConfigLoader.__init__`, `lib/portage/repository/config.py:237-245`/`484-490`). An empty `layout.conf`/`repos.conf` value is a real "no masters", distinct from the key being absent; the main repo can never be its own master. Portage `config.py` only logs a warning for an unknown master, never a hard error. Only absolute locations reach `portage_profile::resolve_config`'s masters-stacking.

## struct RepoConfig::profile_formats
- Portage `parse_layout_conf` (`config.py:1516`); `LocationsManager`'s `_allow_parent_colon = frozenset(["portage-2"])` gate (`_config/LocationsManager.py:47`/`259`). Threaded into `portage_profile::resolve_config` as the set of repo names that allow cross-repo `parent` syntax. Portage's EAPI-conditional default when the key is absent (`portage-1`/`portage-1-compat`) is not modeled; every real Gentoo repo that uses the syntax declares `portage-2` explicitly.
- A profile `parent` line's `reponame:path`/`:path` cross-repo syntax is only expanded for a profile node whose own repo declares `portage-2` here. Absent simply means "no `portage-2`", i.e. colon-parent syntax disallowed for that repo (a documented simplification).

## struct RepoConfig::cache_formats
- Portage `iter_pregenerated_caches` (`config.py:576-605`) yields a database per known name (`pms` -> `metadata/cache`, `md5-dict` -> `metadata/md5-cache`) in list order, ignoring unknown names; `get_pregenerated_cache` (`config.py:607-616`) keeps only the first. Empty when neither directory exists and the key is unset.

## struct RepoConfig::aliases
- Portage `config.py:216-224`/`492-499`; the mismatch escape hatch is `config.py:1121`. `::`-constrained atoms and cross-repo parents both use the canonical name only.
- Deliberately NOT wired into `::alias` atom matching or `alias:path` profile parents -- a documented cut.

## struct RepoConfig::sync_type / sync_uri / volatile / module_specific_options
- Merged across the global `/usr/share/portage/config/repos.conf` and the user `<config_root>/etc/portage/repos.conf` (user wins). A repo the user is likely to hand-edit is "volatile". `module_specific_options` populated from `portage.sync.module_specific_options(repo)` filtered to keys the section actually carries.
- `repos.conf`'s `sync-type` (Portage's `RepoConfig.sync_type`) and `sync-uri` (Portage's `RepoConfig.sync_uri`), merged the same way; `volatile` (Portage's `RepoConfig.volatile`): the explicit `true`/`yes` value, else Portage's heuristic -- `True` unless the location is under `/var/db/repos` and owned by `root`/`portage`.

## fn path_owned_by_root_or_portage
- Portage `config.py:445`; matches Portage's own "fail safe -> volatile" default.

## fn module_specific_keys
- The sync-module-specific `repos.conf` option keys Portage recognizes for a given `sync-type` (`portage.sync.module_specific_options`), in a fixed order so `emerge --info`'s `Repositories:` output is deterministic (Portage iterates a set).

## fn parse_layout_conf
- Out of the ~20 real keys (`sign-manifests`, `manifest-hashes`, `eapis-banned`, `use-manifests`, ...); the rest are real but out of portuale's scope. Every key portuale reads has a real "absent" default.
- Parses a repo's own `metadata/layout.conf` (Portage's `parse_layout_conf`, `lib/portage/repository/config.py:1516`) -- a section-less `key = value` file. Returns an empty map when the file is absent.

## fn read_repo_name_file
- Portage `_read_repo_name` (`config.py:670-688`); Portage falls back to `"x-" + basename` + `missing = True`, but portuale's fallback (the `repos.conf` `[section]` name) is applied by the caller.

## fn find_repos
- Also the order `list_candidates` iterates repos in. The dev machine uses a `repos.conf` directory.

## fn find_repos_quiet
- The VDB backend's make.conf lookup (backlog #318).
- For a process-internal config probe that runs before the CLI's own load (the VDB backend's make.conf lookup, #318): no "Section ... has name different from repository name" line, so the run prints it exactly as often as before the probe existed.

## fn find_repos_impl
- Global path: `<PORTAGE_BASE_PATH>/cnf/repos.conf` (installed at `/usr/share/portage/config/repos.conf`), e.g. the shipped `[gentoo]` `sync-git-verify-commit-signature = true`.
- `RepoConfigLoader._parse` reads every path through `_recursive_file_list` (`repository/config.py:943-950`): same file/dir exclusion and in-place ordering as `package.*`.
- Name resolution is `profiles/repo_name` when present, else the section name; the `layout.conf` `repo-name` override (`config.py:500-505`) and alias prepending (`config.py:492-499`) come after.
- Section-name mismatch (`config.py:1121-1136`): ported faithfully, including the drop (not a soft warning). Pre-existing double-call: `find_repos` runs at the CLI layer (to build `resolve_config`'s inputs) and again inside `resolve_pretend_graph` -- harmless except for the repeated line; backlog #55's internal probes stay quiet so they add no repeats.
- `parse_layout` (`config.py:1564-1577`): Portage keeps an explicit unknown format and `iter_pregenerated_caches` skips it.
- Masters tiers: `config.py:237-245`/`484-490`.
- Without the user-facing "Section ... has name different from repository name" line, for the internal location-keyed lookups (`repo_config_for_location` -- and therefore `repo_masters_for_location` -- and `metadata_transfer_in_config`): the CLI layers already report a mismatch once, and repeating it on every internal config probe is pure noise.
- Portage `RepoConfigLoader` reads the global `<PORTAGE_BASE_PATH>/cnf/repos.conf` (installed at `/usr/share/portage/config/repos.conf`) before the user's, so a user `[section]` key overrides the global one and unset keys fall through (e.g. the shipped `[gentoo]` `sync-git-verify-commit-signature = true`). Read `config_root`-relative, which resolves to that absolute path for a live `PORTAGE_CONFIGROOT=/` run and stays absent under a test / fixture `config_root`.
- Portage `RepoConfig.volatile` (`config.py:423`): explicit `true`/`yes`, else the heuristic -- volatile unless the tree is under `/var/db/repos` and owned by `root`/`portage`.
- Internal probes (`find_repos_impl(.., false)`) stay quiet so the #55 config lookups do not add to the repeats.

## fn repo_usepkg_atoms
- Backlog #373. Portage reads `usepkg-exclude`/`usepkg-include` per `repos.conf` section (`repository/config.py:253-282`), validates with `_find_bad_atoms` (a package name or slot atom, wildcards allowed, no operator, blocker, `::repo` or USE dep), warns and drops the bad ones, removes an atom found in both lists (with a second warning), and `depgraph.py:384-392` appends `::<repo.name>` before folding them into the CLI sets. The `DEFAULT` section is skipped. Portage loads the repo config three times per run, so its warnings print three times (#418); this prints them once per process (`WARNED`). Evidence: `LLM/evidence/2026-10-18-s8/out373/`. A binary without `REPO:` carries `__unknown__`, which `matches_repo` lets pass any `::repo` atom (`dep/__init__.py:3201`).
