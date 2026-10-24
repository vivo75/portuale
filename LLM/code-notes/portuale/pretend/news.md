# crates/portuale/src/pretend/news.rs
GLEP 42 news.

## module
- Ports Portage's `emerge --check-news` (`_emerge/actions.py:3844` -> `portage.news.getUnreadItems` / `NewsManager`).
- A news item lives at `<repo>/metadata/news/<id>/<id>.<lang>.txt` (`<lang>` = `en` here -- `L10N`/`PORTAGE_L10N` not modelled).
- Ports Portage's `NewsManager.updateItems` write-back (`news.py:112-190`): an item not already in `.skip` is (re-)evaluated; once valid+relevant, it's added to *both* `.unread` and `.skip` (permanently -- from then on it's `.skip`-excluded from re-evaluation, even if it later stops matching a live `Display-If-*` restriction). The printed count is Portage's own `N = len(.unread)` (`getUnreadItems`, `news.py:234`). The only thing Portage ever does that removes from `.unread` is `eselect news read`, a separate tool portuale doesn't implement as its own action; `<eroot>/var/lib/gentoo/news/news-<repo>.read` models that tool's effect (an id there is removed from the `unread` set, lazily, on the next `--check-news` run, rather than requiring a Portage `eselect news read` invocation to have rewritten the file directly). Either file is rewritten (sorted, one id per line) only when its set actually changed -- `write_news_state_if_changed`. `Display-If-Installed` atoms are matched in full (version operators, slot/sub-slot) -- see `news_item_relevant` for the remaining narrow cuts.
- The live `NewsSelector`: Portage's `--check-news` relevance over one repo's `metadata/news`, evaluated with the same `news_item_valid` / `news_item_relevant` used everywhere else. `run_check_news` reads each repo's unread ids through one of these rather than inlining the scan, so the director's news slot carries the production traffic; the state files (`.read`/`.skip`/`.unread` under `<root>/var/lib/gentoo/news`) are read here and written back by the caller, mirroring Portage's `NewsManager.updateItems` + `getUnreadItems`.

## struct FilesystemNews
- The repo checkout (`metadata/news` lives under it); the `::reponame` owning the state files.

## struct NewsContext
- What Portage's `NewsItem.isRelevant` passes its non-vdb restrictions (`news.py:406-423`) plus the ownership Portage's `NewsManager.__init__` gives the state files (`news.py:79-84`).
- `profile`: Portage's `NewsManager._profile_path`: `realpath(settings.profile_path)` with the main repo's `<location>/profiles/` prefix removed (the full path when the profile lives elsewhere); `None` with no profile at all.
- `has_profile`: Portage's `settings.profile_path` is set (a `make.profile` directory exists): `count_unread_news`'s gate (`news.py:470-479`).
- `arch`: Portage's `config.get("ARCH", "")`.
- `inst_uid`: Portage's `int(config["PORTAGE_INST_UID"])` (default 0).
- `portage_gid`: Portage's `portage.data.portage_gid`: the host's `PORTAGE_GRPNAME` group (default `portage`), 0 when that group does not exist (`data.py:173-178`). Portage's `grp.getgrnam` also sees NSS sources (LDAP, sssd); portuale reads `/etc/group` only.
- `Display-If-Profile` / `Display-If-Keyword` restrictions match against the profile/`ARCH` (backlog #258).

## struct NewsEvaluation
- The pure result of one `FilesystemNews::evaluate`: the state-file sets as found (`*_orig`) plus the re-evaluated sets the caller writes back when they changed.

## fn FilesystemNews::evaluate
- `.read` (eselect news read) + `.skip` (updateItems' permanent per-item skip list).
- Once relevant, always in `.skip`, so never re-evaluated again even if the item later stops matching a live `Display-If-*` restriction.
- Portage's `getUnreadItems` count is simply `len(.unread)` -- once an item is added, it stays counted on every future run, regardless of whether it would still evaluate as "relevant" (it's `.skip`-permanent, never re-checked). Portage would have rewritten `.unread` at the time the user ran `eselect news read`; portuale applies `.read` lazily each run instead.

## fn unread_news_counts
- Shared by `--check-news` and the GLEP 42 count notice (backlog #196): every repo maps to its `len(.unread)` count (Portage's `NewsManager.getUnreadItems`, `news.py:234`). The `.unread`/`.skip` files are rewritten exactly like `run_check_news` always did.
- Portage's `updateItems` prepares the state directory first, even for a repo with no `metadata/news` (its `listdir` raises after, and `getUnreadItems` turns that into 0). When the preparation fails it returns before touching the news dir, and `getUnreadItems` counts the `.unread` file as it stands (`news.py:206-239`).
- One filesystem selector per repo, evaluated here (the selector is a pure relevance read, like every other director slot).

## fn print_news_notifications
- Ports `portage.news.display_news_notifications` (`news.py:509-528`): a leading blank line, then one line per repo with a nonzero count, then ` * Use eselect news read to view new items.` plus a trailing blank line. The all-zero gate lives INSIDE, like Portage (`news.py:526`, `if news_reader_display`): an all-zero map prints nothing at all, so a future third caller cannot emit a dangling "Use eselect..." block (review Minor #4).
- An all-zero map prints nothing at all, so a future third caller cannot emit a dangling "Use eselect…" block (review Minor #4). Shared by `--check-news` and the backlog #196 count notice so the two can never drift apart.

## fn news_notice_enabled
- Ports `display_news_notification`'s own gate (`post_emerge.py:38`).

## fn display_news_notice_if_any
- Backlog #196: ports Portage's `_emerge/post_emerge.py::display_news_notification` (`post_emerge.py:37-46`), reusing the existing `FilesystemNews` evaluation -- no second news parser. Portage's own function consults neither `--quiet` nor `--ask` (the m185 probe's `--quiet` shape shows the notice surfacing mid-merge through stdio buffering, which is not ported). Returns Portage's `display_news_notification` boolean (Portage's `actions.py:4268-4281` short-circuits on its falsy return). The all-zero gate lives in `print_news_notifications` (like Portage's `news.py:526`), so this calls it unconditionally past the FEATURES gate. Counted via Portage's `count_unread_news` (`news.py:447-506`).

## fn offer_news_reading
- Backlog #231 (c): Portage's `_emerge/actions.py::run_action` (`actions.py:4266-4281`) uses Portage's `UserQuery.query` with the `--ask-enter-invalid` flag, spawns `eselect news read` with stdio inherited, and only the merge-list prompt quits the run. EOF or a read error (backlog #234) prints Portage's `UserQuery.query`'s own `Interrupted.` (`_emerge/UserQuery.py:74-76`) and reports `true` so the caller exits `128 + SIGINT` before `action_build`, exactly like Portage's `actions.py:4270-4281` (whose `sys.exit` never reaches `action_build` either). When `eselect` is missing the spawn raises `OSError` in Portage. `--ask` without a TTY never reaches here (the up-front `stdin_is_tty` gate exits first), matching Portage's `actions.py:3917-3926`; under `--pretend` `ask` is already false, matching Portage's `--pretend` gate around the whole block.
- When `eselect` is missing the spawn raises `OSError`, and Portage prints `Please install eselect to use this feature.` to stderr.

## fn pretend_end_news_notice
- Backlog #196 fix round 1: Portage's `_emerge/post_emerge.py:112-117` -- when the vdb did not change and the run was `--pretend`, `post_emerge` still runs `display_news_notification` before returning. Portage reaches this arm for EVERY `action_build` retval: `run_action` calls `post_emerge` unconditionally (the final `retval = action_build(...); post_emerge(...)` in the build branch, `actions.py:4289-4297`), so a failed `--pretend` resolve prints the notice too -- every `--pretend` exit of the build path below ends with this call. (Usage-error returns above the resolve never reach `post_emerge` in Portage either, so they stay silent here as well.)

## fn merge_failure_news_notice
- Backlog #196 fix round 1: Portage's `post_emerge.py:155` prints the notice regardless of retval (`retval` only feeds `exit_msg`, `:104-108`) -- but only when the vdb actually changed (`:112-117` early-returns otherwise). `changed` is that gate: the caller passes `unmerged.len() < entries.len()` (at least one entry landed, read off the vdb the way `entries_not_merged` already does for the resume list). A total failure with nothing merged -- e.g. a digest failure on the first package -- stays silent, exactly like Portage.

## fn grab_news_state_file
- Ports Portage's `grabfile` (`util/__init__.py`).

## fn write_news_state_if_changed
- Ports Portage's `NewsManager.updateItems` `write_atomic` calls (`news.py:183-200`) with Portage's `apply_secpass_permissions(uid=PORTAGE_INST_UID, gid=portage_gid, mode=0o064, mask=0)`: group `portage`, group read/write and world read on top of the umask mode (a root run lands `0664 root:portage`, backlog #258).

## fn ensure_news_state_dir
- Ports Portage's `updateItems` preamble (`news.py:121-133`): `ensure_dirs(unread_path, uid=PORTAGE_INST_UID, gid=portage_gid, mode=0o074, mask=0)`, then `os.access(unread_path, os.W_OK)`, same early return. Exactly like Portage's `getUnreadItems` reading `len(grabfile(unread))` after the early return.

## fn apply_news_permissions
- Ports Portage's `apply_permissions` with `mask=0` (`util/__init__.py:1145`). Ownership is only changed by a root caller: a non-root caller is Portage's unprivileged mode (`PORTAGE_INST_UID` and the portage group become the caller's own, so the files it creates already match) or, for the state files, Portage's `apply_secpass_permissions` skipping what `secpass < 2` cannot do. One narrow difference remains for the directory: Portage's `ensure_dirs` has no secpass gate, so a non-root caller facing a writable directory it does not own fails the chown and skips the update, where this proceeds. `false` on a failed chown/chmod (Portage's `OperationNotPermitted`).

## fn news_context
- Profile path: `news.py:88-98`.

## fn news_profile_path
- Ports Portage's `NewsManager.__init__` `_profile_path` (`news.py:88-98`): `realpath` of `settings.profile_path` -- `<config_root>/etc/portage/make.profile`, else the deprecated `<config_root>/etc/make.profile` (`LocationsManager.py:119-143`), else none -- with the main repo's `<location>/profiles/` prefix removed. Portage builds that prefix from the configured location without resolving it.

## fn config_profile_dir
- Ports Portage's `settings.profile_path` (`LocationsManager.py:119-143`).

## fn host_group_gid
- Ports Portage's `grp.getgrnam(name).gr_gid` for `portage.data.portage_gid`.

## fn news_item_format
- A `News-Item-Format:` header whose value `fnmatch`es `[12].*` (i.e. starts `1.` or `2.`); Portage's `NewsItem.parse` `fnmatch.fnmatch(news_format, "[12].*")` break-on-first-match. `None` carries "no valid format header" (Portage's `_formatRE` miss -> invalid).

## fn news_item_valid
- Ports Portage's `NewsItem.isValid`: each `Display-If-Installed` atom passes the format-bound `isvalidatom` gate (`DisplayInstalledRestriction.isValid` -> `isvalidatom(atom, eapi=...)` -> `InvalidAtom` -> `self._valid = False`).
- The `eapi="0"` (1.x) vs `eapi="5"` (2.x) split is *not* a `portage_dep` EAPI parametrization -- it is a narrow field gate, "does it parse at all" untouched: Portage EAPI 0 lacks slot deps (`slot_deps` is EAPI >= 1) and USE deps (`use_deps` is EAPI >= 2). No crate EAPI parameterization is added -- Part 3's non-goal is about ebuild-conditional EAPI logic, not this one-off legacy news-format rule.
- Ports Portage's `DisplayProfileRestriction.isValid` (`news.py:398-404`): fails a 1.x item whose `Display-If-Profile:` value holds a `*` anywhere but a trailing `/*` (`_valid_profile_RE`, `^[^*]+(/\*)?$`); a 2.x value is always valid.
- A 1.x Display-If-Installed atom is legal only when it carries no `:slot`/`:slot/sub` (also no `:=`/`:*` slot operator) and no `[use]` (EAPI 0 lacks slot deps and USE deps). A 2.x atom keeps the permissive parse-at-all check (its own grammar is exactly the EAPI-5+ superset `parse_atom` already implements).

## fn news_item_relevant
- Ports Portage's `NewsItem.isRelevant` (`news.py:269-306`). The three kinds (backlog #258 added the last two): `Display-If-Installed` is Portage's `DisplayInstalledRestriction.checkRestriction` (`vardb.match(self.atom)`); `Display-If-Profile` is Portage's `DisplayProfileRestriction.checkRestriction` (`news.py:406-409`): a 2.x item's `.../*` value matches every profile under that prefix (`startswith` of the value minus its `*`); `Display-If-Keyword` is Portage's `DisplayKeywordRestriction.checkRestriction` (`news.py:422-423`): `config.get("ARCH", "")` equals the value, verbatim (so `~amd64` never matches). A malformed atom makes the whole item *invalid* (see `news_item_valid`), not merely unsatisfied. No profile at all matches nothing. Portage's `match_from_list` ignores use-deps, hence the `use_deps_satisfied` post-filter.
- Portage only matches a restriction line that ends in a newline (`_installedRE` and friends end in `\n`); a header on a file's final, unterminated line is the one shape not mirrored here.
- `Display-If-Installed` -- Portage's `DisplayInstalledRestriction.checkRestriction` is `vardb.match(self.atom)`: a full atom match, version operators (`>=cat/pkg-1`), slot/sub-slot (`cat/pkg:2/3`) and all, against every installed version of the atom's `cat/pkg`. A `[use]`-dep is post-filtered against the matched version's own vdb `IUSE`/`USE` (`use_deps_satisfied`).

## fn run_check_news
- Ports Portage's `emerge --check-news` (`_emerge/actions.py:3844` -> `portage.news.getUnreadItems` / `NewsManager`): print the `IMPORTANT: N news items need reading` block if any repo's `.unread` count is nonzero, else ` * No news items were found.` (unless `--quiet`).

## invalid-item reporting (#554)
- `news_item_invalid_lines` mirrors `NewsItem.parse`'s `invalids` list: non-`[12].*` format lines recorded (first valid one stops the scan and drives restriction checks), `(0, "News-Item-Format unspecified")` with no format line at all, then the `Display-If-Installed`/`Display-If-Profile` failures with 1-based line numbers -- using the exact `news_item_valid` predicates, so verdict and report can never disagree. `news_item_valid` delegates to it. `Display-If-Keyword` has no portuale validity predicate: a bad keyword stays a silent verdict gap, not a reported line.
- `evaluate()` prints `!!! Invalid news item: {path}` + `!!!   line {n}: {line}` per invalid item and skips it (never counted, never `.skip`-ed), like `updateItems`.
- Out of scope: the probe env's extra `gentoo` repo count (host-only repo config, not a product gap).
