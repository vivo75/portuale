# crates/portuale/src/pretend/search.rs
`--list-sets` and `--search`.

## fn run_list_sets
- Ports Portage's `emerge --list-sets` (`_emerge/actions.py:3839` -- `writemsg_stdout("".join(f"{s}\n" for s in sorted(...sets)))`). The built-in names are `cnf/sets/portage.conf`'s own `[section]` headers (Portage's `SetConfig._parse` over the bundled default config), skipping the `multiset = true` `[usersets]` generator (its members are the user set *files*, added below), plus every file under `<config_root>/etc/portage/sets/` (Portage's `usersets.multiBuilder`).

## fn defined_set_names
- See `run_list_sets`. Also used by `run_search`'s own set-name matching.

## fn run_search
- Ports Portage's `emerge --search`/`-s` (`_emerge/actions.py::action_search` -> `_emerge/search.py`).
- Output shape is Portage's `search.output()`: `Searching...\n\n`, the `[ Results for search key : KEY ]` header (with Portage's own `\b\b  \n` spinner-erase prefix), one `*  cat/pkg` line per hit (with the verbose `Latest version` / installed-status / `Homepage` / `Description` / `License` block under `-v`), and the `[ Applications found : N ]` footer.
- `--fuzzy-search` (default on): a hit also lands when `difflib::ratio` (a faithful `difflib.SequenceMatcher.ratio()` port) between the key and a package/set name reaches `search_similarity`% (default 80) -- Portage's `search.py` `fuzzy_search_part`. For a `cat/pkg` key the two halves are scored independently and both must pass (Portage's `part_matchers` split). `--regex-search-auto` (default on): a key containing a regex metacharacter (`^ $ * [ ] { } | ?` or `.+`) that compiles is matched as a case-insensitive regex instead of a literal substring (fuzzy is then off, matching Portage's `search.py`); a leading `%` forces regex mode. Regex detection: Portage's `search.py:265-280`; literal fallback is `re.compile(re.escape(key), re.IGNORECASE)`.
- **v1 cuts:** the search index (`--search-index`); `--usepkg` binary-package results; the `bestmatch-visible` mask/keyword filter (the highest version is used, flagged `[ Masked ]` only when its `KEYWORDS` carry no stable/testing token for the profile arch); `Size of files`.
- A case-insensitive substring search of every `category/package` in the configured repos (the package-name half only, unless the key contains `/`), plus defined set names. `-S` / `--searchdesc` also matches each package's `DESCRIPTION`.
- Set names (`search.py:349-357`) are matched with the same `searchre` (never fuzzy), against the last `/` component unless matching categories.

## fn search_best_candidate
- The highest-version candidate (Portage's `bestmatch` before the visibility filter), a version tie broken toward the higher-priority repo (Portage's `portdbapi.xmatch` repo ordering).

## fn search_candidate_visible
- A rough `bestmatch-visible` stand-in: the candidate carries a non-`-`-prefixed `KEYWORDS` token for `amd64` (stable or `~`) -- portuale's fixtures are all `amd64`. Portage runs the full mask/keyword visibility check.

## fn render_ambiguous_search_output
- Ports Portage's `ambiguous_package_name` non-`--quiet` branch (`depgraph.py:12063-12083`): builds a `search` object seeded via `addCP(cp)` for each ambiguous `cp` (not a free-text query -- so, per Portage's `_results_specified`, `output()` skips straight to rendering the given list, no `_iter_search()`/`"Searching...\n\n"` line at all), then calls `s.output()`. Portage's own `verbose` here is always `True` (`search(..., "--quiet" not in myopts, ...)`, and this branch is only reached when `"--quiet" not in myopts` already) -- so the full metadata block always renders, unlike `run_search`'s own `--quiet`-gated `verbose` toggle. Reuses `run_search`'s `search_best_candidate`/`search_candidate_visible`/render shape; same v1 cut that function's doc comment already documents (`Size of files`, the `bestmatch-visible` mask/keyword filter approximation) -- not re-derived here, this is the identical renderer Portage's own single `search.output()` is, just fed a `addCP`-built list instead of a query match.

## fn run_search (allow-list, #571)
- Takes `config_root` and walks `all_cp(repos, &repo_categories(...))`: unlisted categories are invisible to `--search` (Portage `cp_all(categories=settings.categories)`), with no `!!!` anywhere on this path.
