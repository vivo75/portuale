# 02 — The `emerge` command-line surface

All line references are to `rust/portuale/src/emerge_options.rs`
(option/action tables + `lookup`) and `rust/portuale/src/pretend.rs`
(the actual argv parser inside `run`, `pretend.rs:8389`), plus
`rust/portuale/src/ebuild_options.rs` for the `ebuild` applet.

## 2.1 How argv is parsed

1. `--help`/`-h`/`help` anywhere → print `HELP_TEXT` and exit 0, before
   any config load (`pretend.rs:2879` `wants_help`, `:2892` `print_help`,
   `:8390-8393`). This mirrors real `emerge`'s early `myaction == "help"`
   return.
2. Config is resolved (`find_repos` + `resolve_config`), then
   `EMERGE_DEFAULT_OPTS` from the resolved config is **prepended** to
   argv, so explicit argv wins for last-wins options
   (`pretend.rs:8484-8488`, `4456` `args_with_emerge_defaults`).
   `shell_split` (`pretend.rs:4391`) tokenises the variable with
   quote/escape handling.
3. The parse loop (`pretend.rs:8897-10411` approx.) walks tokens:
   - `--opt=value` inline-equals form and `--opt value` separate form
     are both accepted for value options.
   - Repeatable list options (`--exclude`, `--reinstall-atoms`,
     `--useoldpkg-atoms`, `--rebuild-exclude`, `--rebuild-ignore`,
     `--usepkg-exclude/include`, `--buildpkg-exclude`) use
     `action: "append"` semantics: each occurrence contributes a
     space-separated word list.
   - Optional-value flags (`--ask`, `--verbose`, `--quiet`,
     `--deselect`, `--buildpkg`, `--usepkg`, `--getbinpkg`,
     `--keep-going`, `--jobs`, …) accept bare, `=y`/`=n`, `=True`… forms;
     bare means "on".
   - Unknown tokens that parse as atoms/sets go to `atom_args`;
     anything else is an error naming the token.
4. After parsing: `--tree` + `--columns` together is an error ("can't
   specify both"); `--nodeps` forces off complete-graph and
   dynamic-deps; `--rebuild-if-unbuilt` clears rev+ver, `--rebuild-if-new-rev`
   clears ver (real `main.py:958-975` precedence, `pretend.rs:8562-8568`).

`emerge_options::lookup` (`emerge_options.rs:416`) classifies a token
into `Category::{Boolean, Value, Action}` (`:220`) with its canonical
long name; `find` (`:406`) searches one table. The tables below combine
the declared tables with the flags `pretend.rs` parses directly.

## 2.2 Actions (mutually exclusive run modes)

From `ACTIONS` (`emerge_options.rs:387-404`); each maps to a flow in
`03-emerge-flows.md`:

| Action | Alias | Behaviour |
|--------|-------|-----------|
| (none) | — | resolve targets → show merge list → execute (fetch/build/merge) |
| `--pretend` | `-p` | resolve + display only, never execute (parsed directly, not a table action) |
| `--ask` | `-a` | prompt `Would you like to …? [Yes/No]` after display, before executing |
| `--unmerge` | `-C` | remove named packages (`run_unmerge_pretend` `pretend.rs:3950`, `execute_unmerge` `:5054`) |
| `--depclean` | `-c` | remove unneeded packages (`run_depclean_pretend` `:5857`) |
| `--prune` | `-P` | remove all-but-latest slotted versions (`run_prune_pretend` `:5337`) |
| `--clean` | — | alias shape of `--prune --nodeps` (`run_clean_pretend` `:5468`) |
| `--config` | — | run `pkg_config` phase (`run_config_action` `:7789`); ignores `--pretend` |
| `--deselect` | `-W` | remove targets from world file (+ world_sets) (`run_deselect` `:3523`) |
| `--resume` | `-r` | replay `mtimedb` resume mergelist (`run_resume` `:4838`); `--skipfirst`/`--skip-first` drops the first entry |
| `--search` | `-s` | search repo package names/descriptions (`run_search` `:6183`); `--searchdesc`/`-S` includes descriptions |
| `--list-sets` | — | list available package sets (`run_list_sets` `:6099`) |
| `--check-news` | — | unread GLEP-42 news items (`run_check_news` `:6635`) |
| `--info` | — | system/config summary (`run_info` `:7311`) |
| `--sync` | — | sync repositories (delegates to the sync path; see §3.12) |
| `--regen` | — | regenerate `metadata/md5-cache` (`regen.rs:175` `run`) |
| `--metadata` | — | transfer metadata (`--regen`-adjacent cache action) |
| `--version` | `-V` | print version and exit 0 |
| `--moo`, `--status`, `--rage-clean` | — | recognized actions with real-portage joke/legacy behaviour |

## 2.3 Target-atom forms (`atom_args`)

- `category/package` with optional version/operator/slot/use qualifiers
  (`=cat/pkg-1.2.3`, `>=cat/pkg-2*`, `cat/pkg:slot`, `cat/pkg[use]`, …).
- Bare `pkgname` → qualified against the repo's `cat/*` list
  (`qualify_bare_name` `:3833`); ambiguous/versioned/slotted bare forms
  handled by `dep_expand_token` (`:3784`).
- `@world`, `@system`, `@selected`, `@preserved-rebuild`, `@live-rebuild`,
  named sets (`@set`), custom set files (`resolve_custom_set` `:3369`,
  `expand_selected` `:3058`), VDB paths
  (`resolve_vdb_path_arg` `:3862`).
- A lone `-` reads targets from stdin (real-portage convention).

## 2.4 Modifier options (selection of the complete surface)

**Display:** `--verbose`/`-v`, `--quiet`/`-q`, `--tree`/`-t`,
`--columns`, `--alphabetical`, `--unordered-display`,
`--color y|n`, `--cols`, `--quiet-repo-display`, `--nospinner`,
`--verbose-slot-rebuilds`, `--verbose-conflicts`,
`--verbose-missing-ebuilds`, `--quiet-unmerge-warn`, `--quiet-build[=y|n]`,
`--quiet-fail`.

**Resolution control:** `--update`/`-u`, `--deep` (with depth value),
`--newuse`/`-N`, `--changed-use`/`-U`, `--changed-deps`,
`--changed-slot`, `--complete-graph`, `--complete-graph-if-new-use`,
`--complete-graph-if-new-ver`, `--nodeps`/`-O`, `--onlydeps`/`-o`,
`--onlydeps-with-ideps/rdeps`, `--emptytree`/`-e`,
`--noreplace`, `--selective`, `--newrepo`, `--rebuild-if-new-slot`,
`--rebuild-if-new-rev/ver/unbuilt`, `--rebuild-exclude`,
`--rebuild-ignore`, `--reinstall-atoms`, `--useoldpkg-atoms`,
`--exclude`, `--ignore-built-slot-operator-deps`,
`--ignore-soname-deps`, `--ignore-world`, `--dynamic-deps[=y|n]`,
`--implicit-system-deps[=y|n]`, `--package-moves[=y|n]`,
`--misspell-suggestions[=y|n]`, `--binpkg-changed-deps[=y|n]`,
`--use-ebuild-visibility[=y|n]`, `--binpkg-respect-use`,
`--with-bdeps`, `--with-test-deps`, `--root-deps`,
`--autounmask*` family, `--accept-properties/restrict`,
`--backtrack`, `--fuzzy-search`, `--regex-search-auto`,
`--search-similarity`, `--search-index`.

**Execution control:** `--jobs`/`-j N`, `--load-average`/`-l X`,
`--keep-going`, `--fetchonly`/`-f`, `--fetch-all-uri`/`-F`,
`--buildpkg`/`-b`, `--buildpkgonly`/`-B`, `--buildpkg-exclude`,
`--usepkg`/`-k`, `--usepkgonly`/`-K`, `--getbinpkg`/`-g`,
`--getbinpkgonly`/`-G`, `--getbinpkg-exclude/include`,
`--usepkg-exclude/include`, `--usepkg-exclude-live`,
`--oneshot`/`-1`, `--select`/`-w`, `--noconfmem`,
`--fail-clean`, `--skipfirst`, `--digest`, `--rebuilt-binaries`,
`--rebuilt-binaries-timestamp`, `--quickpkg-direct`,
`--quickpkg-direct-root`, `--pkg-format`, `--prefix`,
`--root`, `--sysroot`, `--config-root` (recognized, inert:
config root comes only from `PORTAGE_CONFIGROOT`),
`--jobs-tmpdir-require-free-gb`, `--sync-submodule`,
`--depclean-lib-check y|n`, `--alert`/`-A`, `--debug`/`-d`,
`--read-news`, `--update-if-installed`, `--nobindeps`.

Every entry above is accepted by name (unknown options are rejected
with the offending token quoted, `pretend.rs:9328`). Options that take
no effect in a given mode (e.g. `--ask` under `--pretend`) are
explicitly ignored rather than erroring.

## 2.5 Exit codes

- `0` — success (including a clean pretend with an empty merge list,
  and `--autounmask-only`).
- `1` — resolution failure, fetch/build/merge failure, config error,
  usage error, unrecognized applet/option.
- `130` — user declined at an `--ask` prompt or `No`/EOF
  (`ask_confirm` `:4654`, `classify_yes_no` `:4710`).
- Distinct non-zero codes surface child-phase failures (task exit codes
  propagate through `run_one_phase_bash`); `--keep-going` collects and
  reports all failures at the end instead of aborting at the first.
