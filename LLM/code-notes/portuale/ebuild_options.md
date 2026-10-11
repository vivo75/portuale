# crates/portuale/src/ebuild_options.rs
Portage's `ebuild` CLI option and command surface, recognized but not executed. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## module
- Origin: `bin/ebuild`'s `argparse` setup for the six options, and `lib/portage/package/ebuild/doebuild.py`'s `validcommands` list -- what `doebuild()` actually accepts as a phase/command argument, which is a superset of `lib/portage/const.py`'s `EBUILD_PHASES`: it also includes non-EAPI-phase actions like `clean`, `digest`, `manifest`, `merge`, `qmerge`, `rpm`, `unmerge`, `depend`, `fetch`, `fetchall`, `cleanrm`, and `help`.
- Nothing here is behaviorally implemented -- phase execution is explicitly deferred (see LLM/agent-context.md's "Deferred: ebuild phase execution").
- `ebuild` is tested directly against the compiled binary by `tests/test_portuale.py` (see that file's own doc comment).
- `-h` is purely argparse's own auto-added short form for `--help`; Portage's `bin/ebuild` declares no short aliases for any of its six options. No bundling concept exists for `ebuild` at all (unlike `emerge`'s `-pv`-style bundling), so option lookup is a plain whole-token check. See `ebuild.rs` for the portuale-specific help text.
- `--version` was scoped as an equally simple case before digging in, then deliberately dropped: Portage's `bin/ebuild` own `print("Portage", portage.VERSION)` looks static, but `portage.VERSION` (lib/portage/__init__.py) is only a fixed, build-substituted `"@VERSION@"` string for an *installed* copy -- for an `installation.TYPE == installation.TYPES.SOURCE` checkout (exactly what this repo itself is, confirmed by there being no `lib/portage/VERSION` file anywhere in it), `VERSION` is instead derived live via `git describe --dirty --long --match "portage-*"` against the current commit and working-tree state (verified directly: `git describe` in this repo returns `portage-3.0.81-272-g1cb1941de`, parsed into `3.0.81.dev272+g1cb1941de`) -- host/git-state-dependent, non-deterministic output portuale's pinned-test philosophy already rules out elsewhere (the same reasoning that ruled out `emerge --version`/`-V`'s own `getportageversion()`, which pulls in live python/gcc/libc/uname info -- a different Portage function, same disqualifying trait).

## const OPTIONS
- The six options from `bin/ebuild`'s `argparse` setup. `-h`/`--help` is implemented but excluded for the same reason `emerge_options.rs` excludes its own `--help`/`-h`.

## const COMMANDS
- From `doebuild()`'s `validcommands`, the authoritative set (not just EAPI phase names).
