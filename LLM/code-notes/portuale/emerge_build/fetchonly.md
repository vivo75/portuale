# emerge_build/fetchonly.rs

## module

`--fetchonly`/`--fetch-all-uri` (backlog #401). `run_fetchonly` walks the merge list in order and, for every source entry the scheduler would build (`scheduler_needs_build`): under `--pretend` prints one line per distfile with every candidate URI (`ebuild_phases::list_sources`, the `fetch(listonly=1)` shape: a blank line, the URIs each followed by a space), otherwise prints `>>> Fetching (i of n) cpv::repo[ for ROOT]` and runs the same `fetch_sources` the build path uses. Portage: `Scheduler.py` (fetchonly arm), `EbuildBuild.py:153-175,231-256`, `EbuildFetchonly.py`, `fetch.py` (`listonly`). Behaviour pinned from six Portage cells (`LLM/evidence/2026-10-18-401-s0/`).

- Nothing is written to the vdb, world or resume list: the caller (`pretend::run_after_display`) returns before `merge_setup` (which rotates `mtimedb`'s resume list) and the world record. Portage: `actions.py:271-272`, `mtimedb.py:46,94-95`.
- A failed package prints `!!! Couldn't download '<file>'. Aborting.` to stderr and `\n>>> Failed to emerge cpv[ for ROOT]` to stdout, the run goes on with the next package, the exit code is 1 (`Scheduler.py:1918-1923`).
- The non-pretend run prints one extra blank line before the first job (Portage's output has two after the resolution notice).
- `DISTDIR` is the calling env, then the config, then `/var/cache/distfiles`.
- Not modelled: binary entries (skipped), `FEATURES=mirror`, `--keep-going`/`--resume` (backlog #415).
