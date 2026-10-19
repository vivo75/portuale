# #385 oracle: empty `dodir` directories vs `keepdir` (2026-10-19)

Question (backlog #385, filed 2026-10-17 from 02.355 S5, UNVERIFIED):
does portuale strip an empty `dodir`'d directory from the image where
Portage does not (or vice versa)? Owner hint 2026-10-18: a bare `dodir`
being stripped is expected (Portage's `95empty-dirs` / install QA);
`keepdir` is how a directory is kept (hidden `.keep_<cat>_<pkg>-<slot>`
file); EAPI 8 `strict-keepdir` changes `.keep` handling.

Oracle: Portage 3.0.82.2 on this host ("Portage").
Candidate: this tree's `target/release/portuale` at `3dd86926`
(rebuilt 2026-10-19 before the runs; mtime pinned per cell in `*.env`).

## Method

Hermetic per-cell scratch bases under `/var/tmp/s385` (scripts in
`scripts/`: `setup.sh`, `run.sh`, `cells.sh` — same shape as the #355 S0
and #403 harnesses). One hermetic configroot (`cfg/`): single repo
`t385` (throwaway overlay, `thin-manifests = true` so no Manifest or
md5-cache is needed), minimal `default` profile (`ARCH/ACCEPT_KEYWORDS
amd64`, `ACCEPT_LICENSE="*"`), base `FEATURES="-sandbox -usersandbox
-userpriv -network-sandbox -ipc-sandbox -pid-sandbox -mount-sandbox
-news -merge-wait"`.

Each cell runs a single `ebuild <file> merge` (host `ebuild` for
Portage, `portuale ebuild` for portuale) into a fresh `ROOT` with a
fresh `PORTAGE_TMPDIR`/`DISTDIR`. `FEATURES` for the cell is appended
to the shared `make.conf` for the duration (`mc_set`/`mc_restore`):
`s` cells add `strict-keepdir`, every cell adds `noclean`.
`FEATURES` is incremental and normalized before export
(`config.py`, `prune_incremental`), so `-flag` forms never reach the
QA script's substring test.

Why `noclean` on every cell: Portage's merge tail runs the `clean`
phase after a successful merge unless `FEATURES=noclean`
(`vartree.py:6183`), which deletes the image; `clean` is not
implemented by `portuale ebuild`, so without `noclean` the two sides
would differ in image survival for a reason unrelated to #385.
`noclean` changes no QA behaviour. (Post-merge `find $D` still
differs uniformly by merge mechanics — see §"Uniform non-divergences".)

Matrix: shapes (a) `dodir /opt/t385/empty`, (b) `keepdir
/opt/t385/kept`, (c) `dodir /opt/t385/nest/empty` + `keepdir
/opt/t385/nest/kept`, (d) `dodir /opt/t385/dirempty` + `insinto
/opt/t385/withfile` + a `doins`'d `payload.txt` — each as an EAPI 7
and an EAPI 8 ebuild (`t385-<shape>-<7|8>.ebuild`, `SLOT="0"`), each
with (`s`) and without (`n`) `FEATURES=strict-keepdir`. 16 configs x
2 PMs = 32 merges, all rc 0. Per cell the harness records the merge
argv/env/rc/stdout/stderr, the image tree (`image.find`, plus
`image.empty` = `find $D -type d -empty`), the merged ROOT tree
(`root.find`, minus `var/db/pkg` + `tmp`), and the vdb `CONTENTS`
(`contents`). Raw captures: `cells/`.

Reference semantics (read, not assumed):
`bin/install-qa-check.d/95empty-dirs` (vendored copy byte-identical
to the host's): with EAPI 8+ (`___eapi_has_strict_keepdir`, i.e. EAPI
not in 0-7) or `strict-keepdir` in `FEATURES`, `find $ED -depth
-type d -empty -delete` removes every empty dir bottom-up; otherwise
only `/var` empty dirs draw a QA warning and nothing is deleted.
`keepdir` = `dodir` + one zero-size `.keep_<cat>_<pn>-<slot>` file
per directory (vendored helper byte-identical to the host's), so a
keepdir'd directory is never `-empty`.

## Portage ground truth (all reproduced in `cells/*-pg.*`)

| EAPI | strict-keepdir | bare `dodir` dir | `keepdir` dir (+`.keep` file) |
|---|---|---|---|
| 7 | off | KEPT (a7n, c7n, d7n) | kept (b7n, c7n) |
| 7 | on | STRIPPED, parents up the chain too (a7s: nothing merged; c7s, d7s) | kept (b7s, c7s) |
| 8 | off | STRIPPED (a8n, c8n, d8n) | kept (b8n, c8n) |
| 8 | on | STRIPPED (a8s, c8s, d8s) | kept (b8s, c8s) |

A stripped chain deletes bottom-up: a7s leaves no `/opt` at all in
image, ROOT, or CONTENTS.

## Per-cell verdict (cell x PM)

`agree` = image dirs, merged `/opt/t385` tree, vdb CONTENTS, and merge
stdout/stderr all match (modulo the three uniform non-divergences
below; CONTENTS mtimes differ run-to-run as always).

| cell | shape | EAPI | strict | Portage | portuale | verdict |
|---|---|---|---|---|---|---|
| a7n | bare dodir | 7 | no | `empty/` kept everywhere | same | agree |
| a7s | bare dodir | 7 | yes | stripped, nothing merged | same | agree |
| a8n | bare dodir | 8 | no | stripped, nothing merged | same | agree |
| a8s | bare dodir | 8 | yes | stripped, nothing merged | same | agree |
| b7n | keepdir | 7 | no | `kept/` + `.keep_app-misc_t385-b-0` | same | agree |
| b7s | keepdir | 7 | yes | same as b7n | same | agree |
| b8n | keepdir | 8 | no | same as b7n | same | agree |
| b8s | keepdir | 8 | yes | same as b7n | same | agree |
| c7n | nested dodir+keepdir | 7 | no | `nest/empty` + `nest/kept`+`.keep` | same | agree |
| c7s | nested dodir+keepdir | 7 | yes | `empty` stripped, `kept`+`.keep` kept | same | agree |
| c8n | nested dodir+keepdir | 8 | no | same as c7s | same | agree |
| c8s | nested dodir+keepdir | 8 | yes | same as c7s | same | agree |
| d7n | dodir + sibling file | 7 | no | `dirempty/` + `withfile/payload.txt` | same | agree |
| d7s | dodir + sibling file | 7 | yes | `dirempty` stripped, `withfile/payload.txt` kept | same | agree |
| d8n | dodir + sibling file | 8 | no | same as d7s | same | agree |
| d8s | dodir + sibling file | 8 | yes | same as d7s | same | agree |

## Verdict: NO DIVERGENCE on #385

Both managers strip exactly the cells Portage's `95empty-dirs`
strips and keep exactly what `keepdir`'s `.keep` file protects,
including the nested (c) and sibling-file (d) shapes, on EAPI 7 and
8, with and without `strict-keepdir`. The S5 worker's observation
(`dodir /real` never reaches the image) is confirmed as expected
Portage behaviour, not a portuale bug. No product code changed, no
fixtures added, no pins added (per the verify-first brief).

## Uniform non-divergences (same on all 16 cells, out of scope)

1. Post-merge `find $D` retains merged regular files on portuale
   (`.keep_*`, `payload.txt`) while Portage's image loses them:
   Portage's `mergeme` renames files out of `$D`
   (`vartree.py`, `movefile`); portuale's `merge_tree` copies
   (`ebuild_merge/protect.rs`, `replace_file_atomic`: copy to a
   sibling temporary, then rename over dest — the #96 fix), leaving
   the source intact by design (`tree.rs`: fifos/devs are recreated
   "so `${D}` stays intact"). ROOT trees and CONTENTS are unaffected
   and identical. An install-QA comparison must snapshot the image
   pre-merge (or compare modulo regular files, as done here); this
   is a merge-mechanics property, not an install-QA divergence, and
   is left untouched.
2. Merge stdout is byte-identical modulo scratch-root path and ANSI;
   stderr is empty on both sides for every cell (no `/var` paths, so
   no `eqawarn` empty-dir notices fire anywhere).
3. Fresh-ROOT side noise, identical on all 16 cells: portuale-only
   `./etc/ld.so.conf` (98 B env-update header); Portage-only empty
   `./var/lib/portage/preserved_libs_registry` and `./var/tmp`
   (mode 1777). Unrelated to empty-dir QA; recorded, not pursued.
