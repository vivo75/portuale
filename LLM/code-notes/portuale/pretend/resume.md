# crates/portuale/src/pretend/resume.rs
`--resume`.

## fn entries_not_merged
- The per-entry mapping itself is `emerge_build::resume_cpv` (the same mapping the up-front save uses), narrowed here to entries whose merge never landed. One `stat` of the entry's `CONTENTS` through the root's backend.

## fn unmerged_for_resume_save (#444)
- The failure-path resume list: the vdb-unmerged entries plus any postinst-failed entry (whose merge landed -- Portage keeps a failed entry in the saved list; only a `--keep-going` resume pass, which already re-saved without it, drops it). Entry order follows the plan (Portage's up-front order).

## struct RunResumeArgs
- `quiet`: Portage's `--quiet` (`noiselimit < 0`, `actions.py:3907-3908`).
- `ask` / `read_news`: Backlog #231 (a/c): Portage's `run_action` prints the pre-resolution news notice (and offers the `--read-news` prompt) before `action_build` -- which is where the resume handling lives -- so resumed builds print both notices too.

## fn run_resume
- Ports Portage's `_emerge/actions.py` resume handling. Each saved `cat/pkg-ver` is built + merged through `emerge_getbinpkg::run_merge_plan` in the recorded order and per its own recorded `ResumeEntryKind` (source or binary -- Portage's own resume mergelist records both); `--skipfirst` drops the first (the one that failed). Merges nothing and touches neither the resume list nor `world` under `--pretend`.
- Backlog #231 (a): Portage's `_emerge/actions.py::run_action` (`actions.py:4266-4270`) prints the GLEP 42 pre-resolution notice before `action_build` -- and the resume handling (`resume = True`, `resume_depgraph`, `actions.py:220-...`) lives *inside* `action_build` -- so a resumed build prints it too (even when the resume list turns out missing or empty below: Portage's notice precedes all of that). Gated on non-`--pretend` like Portage; the `--ask` + `--read-news` prompt (c) rides the same pre point.
- An interrupted news prompt (backlog #234) exits `128 + SIGINT` before `action_build`, like Portage's `UserQuery.query`'s own `sys.exit` (`_emerge/UserQuery.py:74-76`).
- Portage reaches `post_emerge` even on the missing/empty-list path (unconditional in `run_action`, `actions.py:4289-4297`): with nothing merged that prints the notice only under `--pretend` (`post_emerge.py:112-117`) -- a no-op otherwise. Same `post_emerge` arm for the empty-mergelist return.
- The local binary pool for resumed `Binary` entries (Portage's bintree re-resolution, backlog #186): the resolver's own pool constructor. Cheap: the constructor reads `<pkgdir>/Packages`, and only scans `$PKGDIR` files when a `--usepkg`-family run already scanned them into `config.scanned_binpkgs`. A resumed binary entry re-resolves its `::repo` from the binary pool, never from the ebuild repos (Portage's `depgraph.py::_loadResumeCommand`).
- The `mtimedb` resume list records only `cat/pkg-ver`, so the resolver's own `use_flags_display` population is skipped. Recomputed for a source build -- `emerge_build::build_use_env` turns it into the phase `USE=`, and an empty one makes a `python-r1`/`distutils-r1` (or any USE-conditional) ebuild `die` in `src_compile`. A binary entry carries its own baked USE (`merge_binpkg`), so it needs nothing here.
- Portage's `--resume` honours the recorded `--oneshot`/`--onlydeps`.
- Portage's `post_emerge()`: the info-dir regen fires after a `--resume` too (the vdb changed -- the failure branch already returned, and an empty mergelist returned even earlier), before the preserved-libs advisory.
- Backlog #231 (a): Portage's `post_emerge()` (`post_emerge.py:155`) prints the GLEP 42 notice a second time once the resumed merge changed the vdb -- same FEATURES-plus-nonzero-count gates as the non-resume success path.
- `emerge --resume [--skipfirst] [--pretend]`: replay `mtimedb["resume"]["mergelist"]` (Portage's `_emerge/actions.py`'s resume handling). With `--pretend`, print the saved list and stop (merging nothing, touching neither the resume list nor `world`). `--skipfirst` drops the first (the one that failed). On success the resume list is cleared and the saved `favorites` are recorded in the world file -- unless the recorded `myopts` carried `--oneshot` / `--onlydeps`, in which case the same suppression the original run would have applied is honoured. On another failure the still-unmerged tail is re-saved with the same `myopts`. Portage's own resume mergelist records both source and binary entries (`ResumeEntryKind`).
- Backlog #453 A: the resume notice and timing print through the same colour gates as the ordinary path (`darkgreen` fetched banner, `timing_line` with no backtrack suffix -- `resume_depgraph` passes no counts, `depgraph.py:12302`).

## fn resume_display_entries (#460)
- Portage re-resolves the saved mergelist (`resume_depgraph` → `_loadResumeCommand` → `_add_pkg`), so `--resume --pretend` renders an already-installed entry `[ebuild R]`. The saved list pins `=cpv`, so one vdb `CONTENTS` check per `New` entry is the whole re-resolution (`U`/slot-rebuild markers are unreachable: the atom admits exactly that version). The merge entries themselves stay `New` -- the retry must rebuild regardless. Oracle `LLM/evidence/2026-10-21-f460/` (host `emerge` 3.0.82.2: lone-postinst `--resume --pretend` shows `[ebuild R]`, partial shows `R` + `N`); that same oracle contradicts the entry's "nothing to resume" premise -- Portage retries the lone failure and fails rc 1, like portuale already did.

## fn resume_pretend
- `emerge --resume --pretend` (Portage's `_emerge/actions.py`: re-resolve the resume list, `mydepgraph.display(...)`, `return os.EX_OK`). Portuale's resume list only records `cat/pkg-ver` (no full re-resolution): installed entries render `[ebuild R]` via `resume_display_entries` (#460), the rest `[ebuild N ...]` -- the remaining divergence from Portage's re-derived markers is only the unreachable `U`/slot-rebuild shapes (the saved atom pins the version), same limitation the resume merge itself already carries. `--resume --pretend` never re-resolves, so it can't know about slot-operator rebuilds (the marker caveat). No resolve ran, so no masked-dependency, USE-unsatisfied or plain-miss disclosure either (any NVC entry here renders the bare line). Backlog #231 (a): Portage reaches `post_emerge` after the `--resume --pretend` display too (`actions.py:4289-4297`), printing the notice when the vdb did not change (`post_emerge.py:112-117`).

## fn resume_merge
- Portage's `Scheduler.merge() -> _save_resume_list()` (#168): together with the per-merge shrink inside the merge loops, the re-save leaves exactly the tail behind if this run is itself SIGKILLed.
- `preinst_mask()`: config-resolved `INSTALL_MASK` + the `no{man,info,doc}` `FEATURES` fold (the env read `from_env` did is the `ebuild <file> merge` fallback -- overridden here like the non-resume `emerge` path does).
- The run-wide phase env and `package.env` vars the non-resume `emerge` path resolves (see `run_wide_phase_env` there): without these a resumed `src_compile` sees `CFLAGS=""`. (The per-entry resolved `USE` already flows via `candidate_use_flags_display` above.)
- `run_merge_plan` has no scheduler of its own yet (a pre-existing, separate gap, not introduced here) -- unlike a pure-source resume, a binary-involving one never had `--jobs` parallelism to begin with, so this isn't a regression for it.
- Backlog #197: `--resume` carries no `--quiet`/`--verbose` in `mtimedb["resume"]` (a documented `myopts` cut), so the display mode is serial-streaming unless `-j` >1. Same for `--quiet-build`: stream, unless `-j` >1.
- The empty-`still` guard is the failure path's own (`write_resume_list` itself is unconditional now, #179).
- Backlog #174: a merge-time binpkg digest failure already printed Portage's full tail -- no `emerge:` line either (Portage exits a merge failure via `FAILURE`, not the action error path). Backlog #187 extends the same silence to the missing-file sentinel (Portage's `BinpkgVerifier`'s ENOENT arm).
- Backlog #231 (a): Portage's `post_emerge.py:155` prints the notice regardless of retval -- gated on the vdb having changed (`:112-117`), which here is `still.len() < entries.len()` (the same structural approximation as the non-resume failure arms' `merge_failure_news_notice`).
- #333: `CONFIG_PROTECT[_MASK]` from the resolved config, as the non-resume `emerge` path does.
- An all-source resume list keeps going through `run_source_merge` (full `--jobs`/`--load-average` scheduler support); a mergelist with at least one resumed binary entry (Portage's own resume mergelist can mix both -- `ResumeEntryKind`) goes through `run_merge_plan` instead, the same per-entry binary-vs-source dispatch `--getbinpkg` itself already uses.

## fn run_resume scope replay (#512)
- The saved `myopts` replays the scope the original run used; unset means Portage's `deep` default.
