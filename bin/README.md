# `PORTING/bin/` — vendored Portage phase runtime

This directory is a **vendored copy of upstream Portage's `bin/`** — the
bash that `portuale` sources and executes for real ebuild phase
execution. It ships with the pilot so `emerge` runs on a host with **no
Portage installed and no Portage checkout**.

Copied verbatim from the ref recorded in `PORTING/3rdparty/repos.toml`'s
`[portage]` entry, *except* `phase-functions.sh`, which carries one local
change (see its own header comment — brush strategy #2).

## What's here

| | |
|---|---|
| `*.sh` | `ebuild.sh` and its whole `source` closure: `isolated-functions.sh` → `eapi.sh`, `version-functions.sh`; `phase-functions.sh`, `phase-helpers.sh`, `save-ebuild-env.sh`, `bashrc-functions.sh`; `misc-functions.sh` (→ `ebuild.sh`); `helper-functions.sh` |
| `ebuild-helpers/` | every `dobin`/`doins`/`emake`/`prepstrip`/… helper (all bash; the `elog`/`newins`/`prepall`/`chown` symlinks preserved) |
| `estrip`, `ecompress` | referenced by `misc-functions.sh`'s `install_qa_check` |
| `*-qa-check.d/` | the `install`/`preinst`/`postinst` QA-check script sets `misc-functions.sh` sources |
| `filter-bash-environment.py` | stdlib-only (no `import portage`); `__filter_readonly_variables` runs it on every phase's env save |

## What's *not* here

The `.py` helpers that `import portage` — `doins.py` (so `doins` /
`newins` / `dodoc` / `newbin` / …), `dohtml.py`, `install.py`,
`xpak-helper.py`, `gpkg-helper.py`, `chmod-lite.py`, `xattr-helper.py`.
They need `lib/portage` on `PYTHONPATH`, so they're still read from a
surrounding Portage checkout (`ebuild_phases::bin_dir()` overlays this
dir on top of the checkout's `bin/`). With no checkout, phases that call
those helpers fail the way a missing binary already does.

## Re-syncing from upstream

When `PORTING/3rdparty/repos.toml`'s `[portage] commit` is bumped: copy
each file here over from the new upstream tree, then re-apply the local
change noted in `phase-functions.sh`'s header. A plain
`diff -r <checkout>/bin PORTING/bin` (ignoring `phase-functions.sh` and
this README) should otherwise be empty.
