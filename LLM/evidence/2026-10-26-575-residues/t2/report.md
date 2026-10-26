# Task 2 report: `mtimedb` in Portage's full shape (#585)

Date: 2026-10-26. Reference Portage 3.0.83 (`3rdparty/portage`, host `emerge --version`).

## S0 (host Portage, scratch `ROOT`s under `/var/tmp`, since removed)

Host guard: `stat -c "%Y %s" /var/cache/edb/mtimedb` printed `1792756554 2601`
before the first probe and after the last one — the host file was never touched.
(Running-root loads read it; unprivileged-mode global updates skip `/` because
it is not writable by uid 1000, `lib/portage/data.py::_unprivileged_mode`.)

- (a) Fresh root, `ROOT=/var/tmp/t2s0-root emerge -p --color=n --oneshot
  sys-apps/baselayout` (rc 0): wrote `var/cache/edb/mtimedb` (`t2/a-mtimedb`).
  Keys `info, ldpath, starttime, updates, version` (sorted), tab indent,
  `"version": "3.0.83"`, empty maps inline (`"info": {}`), file ends `}`
  with **no trailing newline**, mode 0644. Matches the earlier
  `LLM/evidence/2026-10-26-562/mtimedb` oracle byte-shape.
- (b) Pre-existing `{"bogus": 1, "info": {}, "old": {"/x/updates/1Q-2020": 111},
  "cur": {"a": 1}, "updates": {}, "version": "3.0.83"}`: `-p` (rc 0) printed
  `Deleting invalid mtimedb key: bogus` as the first line of **stderr**
  (`t2/b-p.err`; the other two stderr lines are repo-index notices), and the
  rewritten file (`t2/b-mtimedb`) has the five default keys only, with the
  `old` content merged into `updates` (`/x/updates/1Q-2020: 111` kept) and
  `bogus`/`old`/`cur` gone.
- (c) Same `-p` re-run on the steady root: mtime and md5 identical
  (`t2/c-p.argv`); no warning (`t2/c-p.err` holds only repo-index lines).
  `commit` skips the write when the state is unchanged.
- (d) With `{"bogus": 1, "version": "3.0.83"}`: `--info` → warning on stderr,
  info block on stdout, file **not** rewritten (bogus still there);
  `--version` → warning on stderr, version line on stdout; `--help` →
  empty stderr, no warning (`t2/d-*`). `-p` with `ROOT` set (running root
  `/`, clean) prints exactly **one** warning.
- Two-warning count: **source-derived, not probed live.** Portage has no
  running-root override (`create_trees` fixes the running root to `/` when
  `ROOT != "/"`, `lib/portage/__init__.py:498-528`), and planting a stray
  key in `/var/cache/edb/mtimedb` is forbidden — so the
  target-stray-plus-running-stray shape cannot run on this host.
  `load_emerge_config` constructs one `MtimeDB` per root
  (`lib/_emerge/actions.py:3380-3391`), each `_load` warning for its own
  unknown keys, hence two stray-carrying roots warn twice (target first).
  The portuale-side pin asserts exactly that; the single-warning count is
  empirical from (d).

## Files changed (portuale repo, uncommitted)

- `crates/portuale/src/mtimedb.rs`: `load_normalized` (`old`→`updates`,
  drop `cur`, fill defaults, drop unknown keys with sorted warnings);
  `pub fn load_mtimedb(root) -> MtimeDbLoad` (warnings once per root per
  run via a `WARNED` set; every op otherwise loads fresh and silently, so
  each op is a read-modify-write off current bytes); `render_full` (all
  default keys always, `resume` keys only when set, no trailing newline);
  `commit` (skip-if-unchanged, stamp `portuale-<CARGO_PKG_VERSION>`);
  `write_content` (atomic temp+rename, 0644, portage gid — from the old
  `write_sections`, minus the trailing newline and the remove-when-empty).
  All public `read_*`/`write_*` signatures unchanged. Removed:
  `write_sections`, `preserved_parts`, `render_raw_part`,
  `render_resume_part` (now `render_resume_object`), the
  `#[cfg_attr(not(test), allow(dead_code))]` on `write_updates_mtimes`.
- `crates/portuale/src/pretend/mod.rs` (`run`): after option parsing and
  before the global-updates block, `load_mtimedb` for the target root then
  the running root when different, warnings to stderr.
- `LLM/code-notes/portuale/mtimedb.md`: module note rewritten to the
  load/commit model; new `load_mtimedb`/`commit`/retired-writers/test
  sections; stale splice-back/removal claims removed.
  `python3 LLM/tools/code_notes_index.py` clean;
  `fact_coverage.py HEAD` on both Rust files: 0 facts not found.
- `LLM/portage-quirks.md`: Q18 (`mtimedb["version"]` is write-only;
  `portuale-<version>` a deliberate difference).

## Files changed (pmtest repo, uncommitted)

- `pytests-contract-suite/test_emerge_pretend_contract.py`: helper
  `_write_stray_mtimedb` plus three pins (docstrings cite S0 (d) and the
  Portage lines):
  - `test_mtimedb_stray_key_warns_once_and_drops_on_pretend` (`-p`,
    fresh root): stderr exactly the warning; file reduced to the five
    default keys.
  - `test_mtimedb_stray_key_warns_on_info_and_version_but_not_help`:
    `--info`/`--version` warn on stderr and leave the stray key on disk;
    `--help` silent on stderr.
  - `test_mtimedb_stray_key_warns_once_per_root` (`--version`):
    target+running stray → two lines; clean running root → one.

## What each Rust test pins (Portage citations in the test docs)

- `fresh_file_has_portage_default_keys`: exact bytes incl.
  `"version": "portuale-300.83.0"`, no trailing newline
  (`util/mtimedb.py:83-86, 105`; shape S0 (a)).
- `stray_key_is_dropped_with_portage_warning`: load warning + drop on
  commit (`:88-90`; S0 (b)).
- `old_key_becomes_updates_and_cur_is_dropped` (`:77-81`).
- `unchanged_state_is_not_rewritten` (`commit` compare `:94-101`; S0 (c)).
- `warning_printed_once_per_root_per_run` (`actions.py:3380-3391`).
- Updated to the commit model: `managed_top_level_keys_..._drop`
  (was `foreign_top_level_keys_survive_every_write`),
  `info_mtimes_accepts_...` (clearing keeps default keys, file persists),
  `remove_merged_entry_shrinks_...` (`resume` key goes, file persists).

## Commands run with results

- RED: `cargo test --release -p portuale --bin portuale mtimedb` → 4 new
  tests failed, 20 passed (pre-fix).
- GREEN: same filter → 25 passed; `global_updates` 32 passed;
  `info_files` 9 passed; `resume` 20 passed (one first failed only as
  "binary older than source", passed after rebuild).
- `cargo fmt -p portuale` + `cargo fmt --check`: clean.
  `cargo clippy --release -p portuale --all-targets`: 0 warnings.
- `cd ../pmtest && python3 -m pytest pytests-contract-suite/test_emerge_pretend_contract.py -q -k mtimedb_stray_key` → 3 passed, no corpus drift
  (`git status` shows only the test file modified).
- Manual binary checks (scratch ROOTs, since removed): stray `-p` warns
  once on stderr and normalizes the file to full defaults with
  `"version": "portuale-300.83.0"` and no trailing newline; stray
  `--info`/`--version` warn without rewriting; `--help` silent; two
  stray roots warn twice, target first. (Mid-sequence note: the first
  `-p` run consumes the stray key, so `--info`/`--version` must be
  re-seeded per command — same as Portage.)

## Design decisions worth a review glance

1. **No state cache.** The first cut cached one normalized state per root;
   it broke the in-process-setup → spawned-run → in-process-read pattern
   (`pretend_writes_no_resume_list_even_with_a_stale_list_present`: the
   test process's cache hid the subprocess's commit). Every op now loads
   fresh; once-only warnings come from a `WARNED` set. Production
   behaviour is identical (nothing external rewrites the file mid-run),
   and parallel same-root runs keep the old per-op read-modify-write
   convergence.
2. **Sorted warnings.** Portage iterates a `set` difference (hash-random
   order); portuale sorts, for byte-identical runs. Single-key pins cannot
   tell the difference.
3. **`moo`/`status`.** portuale reports those as unimplemented (exit 2)
   from a path after the load point, so a stray-key root would print the
   warning where Portage prints moo art / a status table without loading.
   Pre-existing divergence on the actions themselves; out of scope.

## OWNER QUESTION

None — every behaviour above is settled from Portage source or the host
probes, except the two-warning count, which is derived from
`actions.py:3380-3391` (each root gets its own `MtimeDB`, each `_load`
warns) and pinned portuale-side; flag it if a live two-stray probe is
wanted from the bed container instead.
