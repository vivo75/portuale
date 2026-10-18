# #401 oracle: `emerge --fetchonly` / `--fetch-all-uri` (Portage 3.0.82.2, Python 3.14.8, 2026-10-18)

Captured on the host with `app-misc/hello-2.12.3` (SRC_URI one file), `DISTDIR` and
`PORTAGE_TMPDIR` on scratch dirs, `GENTOO_MIRRORS=http://<eth0 IP>:8080` (the distfiles
proxy). Streams are separate files in this directory.

## Captured behaviour

| Cell | argv | rc | stdout | stderr |
|---|---|---|---|---|
| `out-pf` | `-pf app-misc/hello` | 0 | `\nThese are the packages that would be fetched, in order:\n\n` · `Calculating dependencies ... done!` · `Dependency resolution took N s (backtrack: 0/20).` · `\n\n` · (first time only, a `>>> Downloading '…/layout.conf'` block) · **one line per distfile with every candidate URI, space separated**, mirrors in `random.shuffle` order · news notice | empty |
| `out-f` | `-f app-misc/hello` | 0 | news notice (before the resolution, as on a merge) · `Calculating dependencies ... done!` · `Dependency resolution took …` · `\n\n` · `>>> Fetching (1 of 1) app-misc/hello-2.12.3::gentoo` · `>>> Downloading '<uri>'` + wget progress · ` * hello-2.12.3.tar.gz BLAKE2B SHA512 size ;-) ...  [ ok ]` | `[f1r] Local copy of remote index …` (binhost notice) |
| `out-ffail` | `-f app-misc/hello` with `FETCHCOMMAND=/bin/false` | **1** | … `>>> Fetching (1 of 1) …`, one `>>> Downloading '<uri>'` per candidate, blank, `>>> Failed to emerge app-misc/hello-2.12.3` | `!!! FETCHCOMMAND does not contain the required ${FILE} parameter.` (×2), `!!! Couldn't download 'hello-2.12.3.tar.gz'. Aborting.` |
| (earlier) | `-pf --nodeps`, `-pf -v`, `-pF` | 0 | same shape; `--nodeps` drops the `Calculating dependencies` pair; **no `[ebuild …]` rows and no `[fetch ]` marker in any of them** | — |

`-pf` prints no merge list because `actions.py:541-543` guards `mydepgraph.display()` with
`not (fetchonly or fetch_all_uri)`; the Scheduler then runs the fetch in listing mode
(`EbuildBuild.py:153-175`, `EbuildFetchonly.py:20-36`: `pretend=True` → `doebuild(…, "fetch",
listonly=1)`).

## Source facts (verified in `3rdparty/portage/lib`, byte-identical to the host files)

- Header: `depgraph.py:12096-12097` (`action = "fetched"` for `--fetchonly`/`--fetch-all-uri`),
  printed by `_start_resolution_display` for `--pretend`/`--ask`/`--tree`/`--verbose`.
- Ask prompt: `actions.py:522-523` `Would you like to fetch the source files for these packages?`.
- `-F` implies `-f`: `actions.py:3910-3911`. `FEATURES=mirror` implies `fetch_all_uri`: `Scheduler.py:192-193`.
- mtimedb is read-only under fetch-only: `actions.py:271-272`, `mtimedb.py:46,94-95`; the world
  file is untouched (`:2555-2562`); no vardb/bindb write handle (`actions.py:571-578`).
- Scheduler under fetch-only: scheduling continues after a failure (`Scheduler.py:1918-1923`),
  no `--keep-going` loop (`:1279`), blockers ignored (`:77-79`, `depgraph.py:9268`), `pkg_pretend`
  skipped (`:1744`), strict-manifest pre-check skipped (`:793-796`), parallel-fetch off
  (`:324-327`), no background output (`:81`), no temp-dir check or build-dir lock
  (`EbuildBuild.py:45`); a failed fetch runs `pkg_nofetch` without a build dir and returns 1
  (`EbuildBuild.py:231-256`).

## Known non-determinism

Mirror order is shuffled per run, so URI lines are compared as sets of URIs.

## portuale scope (this plan)

S7.1: `-pf`/`-pF`/`-af` (header, prompt, URI listing, no merge rows). S7.2: `-f`/`-F`
(`>>> Fetching (i of n)`, existing fetch machinery, rc 1 on failure, vdb byte-identical).
Not modelled and recorded as residues if met: `pkg_nofetch`, `FEATURES=mirror`, `--keep-going`
interplay.
