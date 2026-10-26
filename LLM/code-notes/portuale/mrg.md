# crates/portuale/src/mrg.rs
`mrg`: a portuale-only clap front end over portuale's own emerge codepath.

## module
- A from-scratch, plain-spoken re-take on the package-manager front end with deliberately relaxed requirements: `emerge`'s tight constraints do NOT apply to it. It may lean on major mainstream crates (`clap` here, `serde` and friends later) instead of portuale's hand-rolled minimal-dependency machinery; it does not have to live on a minimal static musl box, match Portage's `emerge` byte-for-byte, or stay near-zero-dependency.
- What it shares is Portage `emerge`'s actual option surface: every short and long option keeps Portage's own spelling exactly (authoritative source: `lib/_emerge/main.py`'s `options` list, `shortmapping` dict, `longopt_aliases` dict, `argument_options` dict, and `actions` frozenset), so muscle memory carries over. What may differ is only the *semantics* beyond the names (mostly lining up, where clap allows cheaply).
- `portuale mrg --pretend app-portage/eix` resolves and prints exactly what `portuale emerge --pretend app-portage/eix` does -- the same engine behind a clap front end, keeping clap's parser, help, and exit behaviour.
- The rules `to_emerge_argv` folds into the canonical form (options in definition order, `--long=value` for value kinds, target atoms last):
  - Every `Flag` goes BARE (`--pretend`), never `--pretend=y` -- the emerge codepath's peek-based parser treats a plain Boolean flag and a following (unrelated) token correctly on its own.
  - `Value` and `Append` options the codepath implements go `--long=<value>` (per occurrence for `Append`); ones it does NOT implement yet are forwarded BARE (`--fetchonly`, `--root`, ...) so `pretend::run` names them honestly ("a Portage emerge option, but is not yet implemented in portuale...") instead of rejecting a `--long=value` spelling it never heard of. `emerge_handles` encodes the implemented surface, keyed to the parse-loop section in `pretend.rs`.
  - `OptionalValue`s mirror Portage argparse's inserted literals: the bare forms (`-D`, `-j`, `-l`) carry value `"True"` internally and forward BARE (the codepath's own strict `=` validation would reject `--deep=True`); `--jobs`'s separate-value `y`/`n` unlimited forms also forward BARE (that is their Portage meaning); any numeric value forwards `--long=N`.
- Full option-surface fidelity notes (grounded in main.py, read before deviating):
  - Every plain boolean from `options` + `actions` becomes a clap `ArgAction::SetTrue` flag with its Portage short alias (from `shortmapping`) attached. clap's native short-flag bundling (`-pv`, `-aN`, ...) therefore behaves like Portage's `argparse`. `--cols`/`--skip-first` are Portage's `longopt_aliases` secondary names for `--columns`/`--skipfirst`, modelled as clap `alias`es.
  - `-h`/`--help` is clap's own (Portage declares `help` in its `actions` set, and `-h` in `shortmapping`; clap's built-in help keeps that exact spelling).
  - Deliberate cuts, all within the "full equality is NOT required, keep short and long options as-is" licence:
    - The y/n/`True` *optional-value* options (main.py's own `insert_optional_args`/`default_arg_opts`: `--ask`, `--verbose`, `--quiet`, `--buildpkg`, `--usepkg`, ...) are modelled as plain `SetTrue` flags. That keeps the overwhelmingly common bare forms (`-a`, `-v`, `-k`) AND the `-av <pkg>` / `-pv <pkg>` atom-after-flag spellings working exactly like Portage, instead of clap greedily swallowing the following atom as an (invalid) value -- the one failure mode a `num_args(0..=1)` + choices clap arg would introduce. The explicit `=y`/`=n` forms are not parsed yet.
    - The three `argument_options` with an *optional* numeric value (`--deep`/`-D`, `--jobs`/`-j`, `--load-average`/`-l`) mirror Portage's own `insert_optional_args` semantics exactly: a following token is consumed as the value only when Portage's own validator would accept it (`int(s) >= 0` for `--deep`/`--jobs`, which also accept `y`/`n`, and `float(s) >= 0` for `--load-average`); otherwise the option is bare and gets Portage's own literal inserted default, `"True"`. Achieved by marking the clap arg `require_equals(true)` (so it can never swallow a following atom) plus a small pre-pass (`join_optional_values`) that joins the space-separated valid-value forms (e.g. `-j 4`) into the explicit `--jobs=4` spelling before clap sees them.
    - `--with-bdeps`, `--color`, the y/n choice options, and the other *required*-value choice options keep their required value (with the Portage possible values). A bare `--color` is as much an error here as in Portage argparse.
    - `action: "append"` options (`--exclude`/`-X`, `--buildpkg-exclude`, the `--*binpkg-exclude` / `--*binpkg-include` family, `--reinstall-atoms`, `--useoldpkg-atoms`, `--usepkg-include`) are clap `Append` args, repeatable per the `"append"` action.

## enum Kind
- The y/n optional-value family modelled as a flag is the module-level cut above.

## struct Opt
- `help` is portuale's own wording, not a transcription of main.py's help text.

## const ACTION_IDS
- The `actions` frozenset.

## const OPTIONS
- The `read_news` option is backlog #231 (c): with `--ask`, offer `eselect news read` while calculating dependencies (Portage `_emerge/actions.py:4266-4281`).
- The remote options are portuale-only (see docs/remote-merge.md): `--remote-hostname` selects the remote executor instead of `to_emerge_argv` + `pretend::run`, and any other `--remote-*` without it is a usage error.
- `--solver` is portuale-only (Portage has no `--solver`).
- The VDB backend options are mrg-only (feat#157, backlog #305 S3.1).

## fn emerge_handles
- Whether the emerge codepath (`pretend::run`, the parse loop in `pretend.rs`) has a branch for `long`. Options it handles are forwarded with their value (`--long=<value>`); the rest go BARE so `pretend::run`'s `report_option` names them by their Portage spelling ("a Portage emerge action/option, but is not yet implemented in portuale") rather than rejecting a `--long=value` form it never saw.
- The list is keyed to the parse-loop sections in `pretend.rs` (actions, flags, value options, optional-value numerics, append options) and the `lib/_emerge/main.py` option table that `emerge_options.rs` pins; section line numbers recorded there at the time of writing.
- S7 (#341): `--getbinpkg-exclude`/`--getbinpkg-include` joined the handled append options (the codepath parses and validates them now); the old lossy BARE forwarding is gone. `--rebuild-exclude`/`--rebuild-ignore`/`--usepkg-exclude` have no top-level clap option, so they only ever arrive via the raw `emerge` argv, not through this forwarding.

## fn to_emerge_argv
- Every emitted token either starts with `--` or is an atom, so the codepath's `-x y` optional-value peek can never consume an unrelated token.
- Remote options never reach the emerge codepath (remote mode runs its own executor; `--pretend` drops them silently -- plan docs/remote-merge.md says pretend ignores `--remote-*`). `--vdb-backend` / `--vdb-path` are consumed by `setup_vdb` before `pretend::run`, which never sees them.
- The `OptionalValue` bare/inserted-literal and `-j y|n` unlimited forms forward BARE because the codepath's strict `=` validation would reject `"--deep=True"` / `"--jobs=y"`, and bare `--jobs` is precisely what `-j y` means on the Portage side. Not-implemented optionals forward BARE too (unreachable for the three current ones, but the rule stays uniform).

## fn join_optional_values
- Portage emerge's `insert_optional_args`, trimmed to just what this parser needs: `--deep`/`-D`, `--jobs`/`-j`, `--load-average`/`-l` consume the *separate* following token as their value only when it passes Portage emerge's own validator, and never otherwise. Since the clap arg is `require_equals(true)`, that join has to happen here: a bare `-D cat/pkg` must stay `-D` (bare, `"True"`) with `cat/pkg` as an atom, and `-j 4` must become `--jobs=4`. Attached short forms (`-j4`, `-D2`, `-l2.5`) are joined to their explicit long form too, since require_equals also rejects those. Every other token passes through untouched.

## fn valid_integer / fn valid_integer_or_y_or_n / fn valid_float
- Portage's `valid_integers` / `valid_integers_or_y_or_n` / `valid_floats` from `insert_optional_args`; `valid_float` compares non-finite results against 0 like Python's `float(s) >= 0` does.

## fn run
- A value starting with `-` needs the `=` form for every other `Value` option too: clap never swallows a following flag-looking token.
- After the run, `portage_vdb::reset()` drops the registered backends: redb records a clean close only when its last handle drops, and the registry is a static that never would. Without this, the next read-only open (`--pretend`, `portuale vdb status`, a `portageq` outside mrg) refuses the file as "not closed cleanly" until a read-write open repairs it.

## VDB backend selection
- Feature feat#157, backlog #305 S3.1. Precedence question Q1 (default path) and Q2 (explicit only, no auto-detect).
- `resolve_vdb_selection` only calls `conf` (the make.conf lookup) when flag and environment leave the choice open.
- `open_vdb_backend` never creates a database; a missing file is refused with the convert hint.
- `pending_entries_warnings` design §9.1: the pending entry of mrg's own merge (legitimate while it runs) is never listed because the check runs before any merge of this run.
- `select_vdb` is backlog #318: the choice is always exported, `files` included, so an ebuild phase's `portageq` never resolves make.conf itself and every process agrees on one database. The parent pipe is S6.3 / owner question Q5 (redb only).
- `make_conf_lookup` resolves via `load_repos_and_config`: make.globals, profile chain, make.conf under `PORTAGE_CONFIGROOT`, `other_vars`.

## tests
- `read_news_forwards_bare`: backlog #231 (c).
- `remote_options_parse_for_the_remote_executor`: a value starting with `-` needs the `=` form.
- `remote_consistency_and_defaults`: the placements labelled Slice-6 (vdb/edb defaults and sides, ledger-dir, protect explicitness) and Slice-8 (binary placement: shipped file, absolute dir, hostname required).
- `make_conf_backend_reaches_emerge_ebuild_and_portageq`: backlog #318.
- `redb_merge_phases_query_the_parent_over_the_pipe`: feat#157 S6.3 end to end; `has_version` semantics (the new instance is published between `pkg_preinst` and `pkg_postinst`).
- `a_held_redb_file_is_busy_in_process`: a genuine cross-process refusal.

## const OPTIONS (347-MRG)
- Backlog #347 S8: `--rebuild-exclude`, `--rebuild-ignore` and `--usepkg-exclude` are repeatable atom lists (`action: "append"` in Portage `_emerge/main.py:565-594`, same help shape as `--exclude`), forwarded by `to_emerge_argv` as one `--long=<value>` per occurrence. The emerge codepath already parsed and validated all three (`args.rs`, `cli_atoms.rs` strict families); only the `mrg` table and `emerge_handles` lacked them. Unit-pinned (`rebuild_and_usepkg_exclude_forward_like_exclude`) and pmtest-pinned (`test_mrg_forwards_rebuild_and_usepkg_exclude`).

## tests: shared-fixture hermeticity (Task 11)
- Any test that runs `mrg`/`emerge` with `ROOT` (or `PORTAGE_RUNNING_ROOT`) at the shared fixtures mutates them now: the updates run moves vdb entries and records mtimes on the target (and running) root. `pretend_plan_is_identical_on_files_sqlite_and_redb` copies the whole tree to scratch and serves it as `ROOT`; the missing-database and redb-busy tests use fresh tmp target roots; the `has_version`/`emerge -p` probe splits its shared `run` closure per root (fixtures stay for the read-only `portageq`/`ebuild` calls, scratch copies for `emerge`). A warm-up run establishes the record so the compared backend runs all skip the block.
