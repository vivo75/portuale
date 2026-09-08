# Normalisation ruleset

The rules that `normalize.py` (filesystem/VDB) and `resolve-compare.py`
(resolver output) apply before diffing, so that *legitimate*
Portuale-vs-Portage differences do not show up as findings. Anything
**not** listed here is a hard diff.

This file is the human spec; the code is the authority. Keep them in
sync — a change to one is a bug until the other matches.

## Resolver output (`emerge -pv`, L0)

Applied by `resolve-compare.py`:

- ANSI escapes stripped (belt-and-braces; `--color=n` is passed anyway).
- Trailing whitespace stripped; the `[type  FLAGS ]` inner spacing
  collapsed to single spaces before comparing the flags field.
- `::repo` suffix on a `cpv` is split off and compared separately (a
  `repo` mismatch is its own note, not a `version`/`missing` finding).
- Package identity is keyed by `(type, category/pn)` — a different
  version of the *same* `cp` is a `version` finding, not
  `missing`+`extra`.
- `USE="…"` / `PYTHON_TARGETS="…"` / `ABI_X86="…"` etc. values are
  compared as **sorted token sets** per key, so ordering never matters
  (real and portuale already agree on order per the contract suite, but
  the set compare removes a whole class of false positives).
- Error/message lines (`emerge:`, `!!!`, `* ERROR`, anything mentioning
  `REQUIRED_USE`) have `/var/tmp/portage/<…>` → `<builddir>` and
  `YYYY-MM-DD` → `<date>`, then whitespace-collapsed, before the
  set compare.
- Informational lines (`>>>`, "Size of downloads", per-package disk
  figures, the dependency-graph header/footer) are ignored entirely.
- `Total: N packages` — only the integer `N` is compared (`totals`).

## Filesystem manifest (`snapshot.sh` output, L1+)

Applied by `normalize.py` before `diff.py`:

- **mtimes** are never in `files.tsv`; the parallel `mtimes.tsv` feeds a
  separate, non-fatal `MTIME` check.
- Regenerated caches — compare **presence**, not bytes:
  `/etc/ld.so.cache` (plus a `ldconfig -p` soname-set check),
  `**/__pycache__/**` and `*.pyc`/`*.pyo`,
  `/usr/share/info/dir`, `/usr/share/mime/**`, GTK/icon-theme caches
  (`gtk-update-icon-cache`, `icon-theme.cache`),
  `/usr/lib*/gio/modules/giomodule.cache`,
  fontconfig caches under `/var/cache/fontconfig` (already pruned),
  `/usr/share/fonts/**/fonts.dir`/`fonts.scale`,
  `ca-certificates` bundle + hash symlinks.
- `.keep` / `.keep_<cat>_<pn>-<slot>` files — presence only.
- `/etc/.pwd.lock`, `/etc/.updated`, `/var/lib/portage/.keep*`.

## VDB entry (`/var/db/pkg/<cat>/<pf>/`, L1+)

Applied by `normalize.py` to the unpacked `vdb.tar`:

- Blanked fields: `BUILD_TIME`, `BUILD_ID`, `COUNTER`, `INSTALL_TIME`,
  `PKGDIR` build metadata timestamps.
- `NEEDED`, `NEEDED.ELF.2`, `REQUIRES`, `PROVIDES` — sorted line-wise.
- `environment` (bzip2 → plain): drop `SRANDOM`, `EPOCHREALTIME`,
  `EPOCHSECONDS`, `SECONDS`, `BASHPID`, `BUILD_TIME`, `PPID`,
  any `/var/tmp/portage/<cat>/<pf>-<n>/` path → `<builddir>`,
  `T=`, `WORKDIR=`, `HOSTNAME`, `SANDBOX_*` pids.
- `repository` — trailing whitespace only.
- `CONTENTS` — the `obj` lines carry `<path> <md5> <mtime>`; the
  `mtime` field is blanked, the `md5` kept (a real diff). `dir`/`sym`
  lines compared verbatim (sym also carries an mtime → blanked).
- File ordering inside the entry dir is irrelevant (compared as a map).

## `Packages` binhost index (L1+)

- Blanked: `MTIME`, `BUILD_TIME`, `BUILD_ID`.
- Stanzas sorted by `CPV`; blank-line spacing normalised.

## gpkg / xpak archive (`gpkg-diff.sh`, L2)

- Unpacked; then `build-info/{BUILD_TIME,BUILD_ID,COUNTER}` removed,
  `build-info/environment` normalised as above, embedded `Manifest`
  hash lines dropped, all member mtimes ignored.
