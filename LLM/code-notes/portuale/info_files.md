# crates/portuale/src/info_files.rs
Regeneration of stale GNU info directory indexes after a vdb-changing merge.

## module
- Ports Portage's `chk_updated_info_files` (lib/portage/util/_info_files.py:14-141), called from `_emerge/post_emerge.py::post_emerge` (post_emerge.py:126-130).
- The port is line-by-line in spirit:
  - Gated on the *host* `/usr/bin/install-info` existing (not `ROOT`-relative): without it the whole function is a silent no-op (Portage line 15 -- not even the blank line is printed).
  - `inforoot = normalize_path(root + z)` per non-empty infodir (line 21); skipped when it is not a directory or contains an entry starting with `.keepinfodir` (lines 22-24); regenerated only when its `st_mtime` (whole seconds) differs from `prev_mtimes[inforoot]` or is absent there (lines 25-27).
  - Output via Portage's `EOutput` (output.py:663-676,649-661): a bare `\n` on stdout, then ` * <msg>` einfo lines on stdout / eerror on stderr, each rendered here through the same `color.c("INFO"|"ERR", " * ")` helper `elog::echo_summary` uses. `--quiet` (Portage `noiselimit < 0`, actions.py:3907-3908) suppresses the einfo lines AND the bare `\n` (Portage's `writemsg_stdout` defaults to `noiselevel=0`, which `--quiet`'s `noiselimit=-1` filters); the eerror line and the collected error text (`writemsg_level(errmsg, level=ERROR, noiselevel=-1)`, always to stderr since `ERROR >= WARNING`) print regardless.
  - The regeneration loop (lines 38-134): sorted `listdir`, skip dot-files, subdirectories, and `dir{,ext}{,.old}` names (the `dir_extensions` tuple); on the first processed file rename every `dir<ext>` to `dir<ext>.old`; run `/usr/bin/install-info --dir-file=<inforoot>/dir <inforoot>/<x>` with `LANG=C LANGUAGE=C` (no shell), stdout+stderr merged; classify the output (contains `already exists, for file \`` or starts with the 44-char `install-info: warning: no info dir entry in ` prefix -- both verified live against install-info (GNU texinfo) 7.3 -- as harmless, anything else as an error); restore the `.old` files when no new `dir` was produced; unlink every `dir<ext>.old`; store the new `st_mtime` in `prev_mtimes` (line 134, re-statted after the regen).
- Two deliberate narrowings:
  - Portage merges the child's stderr into its stdout pipe (`stderr=STDOUT`, interleaved); portuale captures both and concatenates stdout-then-stderr. Only a substring test and a prefix test ever read the text, and both behave the same on the concatenated form for every output install-info 7.3 was observed to produce (silent success; the warning prefix first; `already exists` anywhere).
  - Portage checks writability with `os.access(inforoot, os.W_OK)`; the port is `libc::access(W_OK)` on the same path (same syscall, same real-UID semantics -- Portage uses the default `effective_ids=False`, and `access(2)` likewise checks the real UID; `libc` is already a direct dependency).

## const INSTALL_INFO
- Portage `_info_files.py:76-81`.

## const DIR_EXTENSIONS
- Portage `_info_files.py:38`'s own `dir_extensions` tuple, in order.

## const ALREADY_EXISTS
- Portage `_info_files.py:95`.

## const NO_DIR_ENTRY_PREFIX
- Portage `_info_files.py:101`; verified live against install-info (GNU texinfo) 7.3.

## fn normalize_path
- Ports `portage.util.normalize_path` (util/__init__.py:139-153).

## fn is_writable
- Ports `_info_files.py:46`'s own `os.access(inforoot, os.W_OK)` gate; real-UID semantics on both sides. Delegates to the one copy, `privileges::path_writable`.

## fn skippable_name
- Ports `_info_files.py:57-64`.

## fn classify_output
- Ports `_info_files.py:95-110`.

## fn run_install_info
- Ports `_info_files.py:86-87`'s own `OSError -> myso = None`; Portage decodes with `.decode("utf-8", "replace").rstrip("\n")`.

## fn fs_err
- Covers the filesystem operations Portage lets raise (`listdir`/`stat` outside the tolerated `ENOENT` renames/unlinks).

## fn chk_updated_info_files
- Portage's `chk_updated_info_files(root, infodirs, prev_mtimes)`: `prev_mtimes` is Portage's `mtimedb["info"]` (mutated in place -- the caller commits it); `quiet` is Portage's `--quiet` (`noiselimit < 0`). Output goes to the two sinks (production passes locked stdout/stderr; tests pass `Vec<u8>` buffers so the `--quiet` suppression itself is pinned, not just the memo side-effect).
- Eight params (Portage's three + quiet/color/binary + the two sinks) -- arity-lint noise, not a design smell.
- Up-to-date branch: `writemsg_stdout("\n")` (noiselevel 0, `--quiet` filters it) + the einfo; `let _ =` matches `println!`'s own error-ignoring semantics: `print_to` discards write errors.
- The spawn step is Portage lines 75-94: spawn, merged output, `OSError -> None`.
- The error line is Portage's `out.eerror(...)`: `ERR`-coloured ` * ` on stderr; the collected error text is `writemsg_level(errmsg, level=ERROR, noiselevel=-1)`: stderr, always (even `--quiet`).
- The `""` guard mirrors Portage line 43 though `normalize_path` never returns it.
- `Path::exists` follows symlinks, like `os.path.exists`.
- `EOutput` here is never quiet -- the `--quiet` suppression is purely the `noiselimit` checks around the einfo calls.

## fn post_merge_info_update
- Ports `_emerge/post_emerge.py::post_emerge`'s own info block (post_emerge.py:89-130): `infodirs` re-read after `env-update` via the same env.d collation Portage's `settings.reload(); settings.regenerate()` derives `${ROOT}/etc/profile.env` from; the equivalent of Portage's `mtimedb.commit()` (which itself only writes on change). The vdb-changed / `--pretend` / vdb-lock gates live with the callers: every call site is a post-merge sequence reached only after a successful non-pretend merge/unmerge (see `pretend.rs`); portuale is a single process with no `vardbapi` lock to take, and a merge that succeeded already proved the vdb writable. Skipped when `"noinfo" in FEATURES` (post_emerge.py:127 is `"noinfo" not in settings.features`). Production sinks are locked stdout/stderr (same bytes `println!` / `eprintln!` emitted before the sink refactor).

## tests
- `regen_records_the_dir_mtime_and_cleans_the_old_files`: Portage `_info_files.py`'s own contract: after the post-merge step `dir` is regenerated containing the entry, `dir.old` is gone, and `mtimedb["info"][<abs inforoot>]` is the dir's mtime.
- No colour in tests: `color.c` passes text through when the style is disabled, so assertions read the plain Portage strings.
- Portage's own `GNU info directory index is up-to-date.` branch: nothing regenerated, nothing counted -- and the line itself reaches stdout.
- `--quiet` (`noiselimit < 0`): the einfo lines and the bare `\n` are suppressed, while the regen itself -- and its memo record -- still happens.
- Portage `post_emerge.py:127`: `"noinfo" not in settings.features` gates the whole block.
- Portage `_info_files.py:22-24`: a dir containing an entry starting with `.keepinfodir` is never regenerated.
- Portage `normalize_path(root + z)`: `"/" + "/usr/share/info"`.
- `write_info_pair`: the same shape the pmtest live-merge contract test installs through the `dev-libs/dirregenpkg` fixture ebuild.
- `quiet_regen_prints_nothing_but_records_the_memo`: the sinks pin the suppression directly (the pmtest contract test pins the non-quiet lines end to end through portuale's own binary).
- `missing_install_info_is_a_silent_no_op`: Portage `_info_files.py:15`; exercised against a bogus path so the test holds whether or not the host ships install-info.
