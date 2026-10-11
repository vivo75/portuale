# crates/portage-profile/src/profile_chain.rs
The profile inheritance chain: `parent` files, per-profile `make.defaults`, cross-repo `reponame:path` expansion.

## fn process_lines
- Wraps the same key dispatch as `process_make_conf_file` without `source` support.

## fn repo_containing
- A repo location that does not exist on disk (e.g. in a test fixture) simply never matches a real profile dir either way.

## fn repo_profile_formats
- Same "duplicate a small, stable chunk rather than share across a dependency edge that only runs one way" precedent as the `keyword_masked_only`/`is_visible` duplication in `portage-repo`.
- Finds which of `repos` (each `(name, location)`) `dir` lives inside, via the longest matching location prefix -- mirrors Portage `LocationsManager._addProfile`'s own `intersecting_repos`/ `max(key=len)` logic, needed to resolve a same-repo `:path` profile parent shorthand. Repo locations are canonicalized before comparing (falling back to the raw path if that fails, e.g. a repo location that doesn't exist on disk in a test fixture -- it simply won't match any Portage profile dir either way), since `dir` itself is always already canonicalized by `visit_profile`. The `profile-formats` token list from a repo's own `metadata/layout.conf` (Portage `parse_layout_conf`), or an empty list when the file/key is absent. A tiny dedicated reader: `portage-profile` can't depend on `portage-repo` (where the fuller `parse_layout_conf` lives), and this is the only `layout.conf` key this crate needs -- the same "duplicate a small, stable chunk rather than share across a dependency edge that only runs one way" precedent `keyword_masked_ only`/`is_visible` already set in `portage-repo`.

## fn expand_parent_colon
- Portage only allows the colon syntax when the current profile node's own repo declares `profile-formats = portage-2` in `layout.conf` (Portage `_config/LocationsManager.py:47`/`259`'s `_allow_parent_colon = frozenset(["portage-2"])` gate); `parent_colon_repos` is the set of repo names that do (built from `RepoConfig::profile_formats` at the CLI boundary).
- Portage `if not abs_parent and allow_parent_colon:` gate (`_config/LocationsManager.py:207`/`259`): `allow_parent_colon` defaults `True` and is only overridden when the current node intersects a known repo.
- Portage `repositories.get_location_for_name`: canonical names and aliases share one namespace, and an alias never shadows a canonical name (Portage `config.py`'s alias-registration loop skips an already-taken name).
- Note: in the old code the `repo_containing` doc block was accidentally fused with `repo_profile_formats`' (no blank line between them, so both documented that function and `repo_containing` had none); the rewrite gives each its own doc.
- Expands a profile `parent` file line's Portage cross-repo `:path`/ `reponame:path` syntax (`LocationsManager._expand_parent_colon`): a `:` with nothing before it means "this same repo" (`current_repo`), anything else before the `:` is another repo's own name, looked up in `repos`. Both forms expand to `<repo_location>/profiles/<rest>`. A line with no `:` at all (the plain relative-path form) is returned unchanged. Portage only allows this syntax when the *current* profile node's own repo declares `profile-formats = portage-2` in `layout.conf` (Portage `_config/LocationsManager.py:47`/`259`'s own `_allow_parent_colon = frozenset(["portage-2"])` gate) -- `parent_colon_repos` is the set of repo names that do (built from `portage_repo::RepoConfig::profile_formats` at the CLI boundary). A `parent` line containing a `:` whose current repo isn't in that set is returned *unchanged*, matching Portage: the `:` then becomes a literal path component that fails to resolve downstream exactly as it would there.

## fn read_repo_make_defaults_use
- Portage `config.py`'s `_repo_make_defaults`. `${USE}` self-reference resolves to whatever `make.conf`'s `USE=` last set: a documented approximation, since Portage's own `_repo_make_defaults` expand context differs.
