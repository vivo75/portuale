# `porttest` — synthetic merge/packaging test overlay

Tiny, fast-building `EAPI=8` ebuilds, one per isolated merge/packaging
behaviour. `TEST/create-container.bash` bakes this into
`localhost/test-portuale` at `/var/db/repos/porttest`; the L1
orchestrator *also* live-mounts it (`-v …:/porttest-overlay:ro`) so no
image rebuild is needed to iterate — `layers/l1/{build,consume}.sh`
stage it to `/var/db/repos/porttest` + write `repos.conf/porttest.conf`
whenever the atom list has `porttest/` atoms.

Run: `TEST/run/l1-merge-from-binpkg.sh TEST/atomlists/l1-porttest.txt`.
Portage builds each fixture from source into `$PKGDIR`; Portage and
portuale each merge that binpkg; `diff.py` compares. All build in
seconds (no `SRC_URI`).

`metadata/md5-cache/` is committed (generated with `egencache --repo
porttest --update` from the pinned portage) because real Portage parses
a cache-less repo but portuale's reader
(`portage-repo/src/lib.rs::read_md5_cache`) only consults the cache —
without it the L2 `builder-portuale` cannot resolve any `porttest/*`
atom (finding `l2-no-md5-cache-ebuild-fallback`). Keep the cache in
sync when an ebuild here changes (`egencache --repo porttest --update`).

## Shipped (slice 3)

| pkg | exercises |
|---|---|
| `porttest/setuid` | `4711` / `2755` / `1750` binaries + `0600` data (perm preservation through a binpkg merge) |
| `porttest/hardlinks` | hardlinked regular files (`CONTENTS` `obj` dedup, link count preserved) |
| `porttest/symfarm` | relative / absolute / dangling / two-hop-chain symlinks + a bin→system-path link + 20 uniform links |
| `porttest/emptydirs` | `keepdir` (+ `.keep_<cat>_<pn>-<slot>`), nested keepdir, a bare owned empty dir with no keepdir, an `0700` dir |
| `porttest/docs` | `dodoc -r` tree → docompress, `newdoc`, `doman` (compressed), `doinfo` (not), `docinto html` (not) |
| `porttest/installmask` | files an `INSTALL_MASK` / `*.la` / `*.log` strip should drop at merge, incl. a dir that becomes empty once its only file is dropped |
| `porttest/phases` | every `pkg_*` phase appends `<phase> eapi=… ebuild_phase=… merge_type=…` to `/var/lib/porttest/phase.log` — merge runs `pkg_setup`/`pkg_preinst`/`pkg_postinst`, so those lines (and order) must match |
| `porttest/splitdebug` | `FEATURES=splitdebug`: `/usr/lib/debug/**/*.debug` + `.build-id/**` symlinks for a binary AND a shared lib (soname) |
| `porttest/unicode` | filenames with spaces, tabs, `$`, UTF-8, `()[]`, `#%`; a symlink with spaces in name+target; a subdir with a space |

## Not yet (follow-ups)

`cfgprotect` (needs the reinstall sub-case), `soname-1`/`-2` +
`slotdep-*` (L5 preserve-libs / slot-op rebuild), `collision` (aborts a
merge — needs its own harness), `config-script` / `preserve-fail`
(L5), `bigfile`.

Keep each ebuild's `src_install` trivial and its build near-instant.
