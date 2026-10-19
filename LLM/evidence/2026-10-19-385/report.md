# r385: backlog #385 verify-first — no divergence

Task: backlog #385 (`LLM/backlog-tasks-2026-10.md` ~line 201): does portuale
strip an empty `dodir`'d directory from the image where Portage does not
(or vice versa)? EAPI 7+8 x strict-keepdir on/off x shapes (a) bare dodir,
(b) keepdir, (c) nested dodir+keepdir, (d) dodir + sibling doins'd file.
`portuale ebuild <file> merge` vs host `ebuild ... merge`, hermetic
configroot + fresh ROOT per cell. Evidence: `LLM/evidence/2026-10-19-385/`
(`SUMMARY.md`, `scripts/`, `cells/`).

## Per-cell table (Portage 3.0.82.2 vs portuale @ 3dd86926, all rc 0)

| cell | EAPI | strict | shape | Portage result | portuale | verdict |
|---|---|---|---|---|---|---|
| a7n | 7 | no | bare dodir | `empty/` kept (image, ROOT, CONTENTS) | same | AGREE |
| a7s | 7 | yes | bare dodir | stripped, nothing merged | same | AGREE |
| a8n | 8 | no | bare dodir | stripped, nothing merged | same | AGREE |
| a8s | 8 | yes | bare dodir | stripped, nothing merged | same | AGREE |
| b7n | 7 | no | keepdir | `kept/` + `.keep_app-misc_t385-b-0` | same | AGREE |
| b7s | 7 | yes | keepdir | kept + `.keep` file | same | AGREE |
| b8n | 8 | no | keepdir | kept + `.keep` file | same | AGREE |
| b8s | 8 | yes | keepdir | kept + `.keep` file | same | AGREE |
| c7n | 7 | no | nested dodir+keepdir | `nest/empty` + `nest/kept`+`.keep` | same | AGREE |
| c7s | 7 | yes | nested dodir+keepdir | `empty` stripped, `kept`+`.keep` kept | same | AGREE |
| c8n | 8 | no | nested dodir+keepdir | `empty` stripped, `kept`+`.keep` kept | same | AGREE |
| c8s | 8 | yes | nested dodir+keepdir | `empty` stripped, `kept`+`.keep` kept | same | AGREE |
| d7n | 7 | no | dodir + sibling file | `dirempty/` + `withfile/payload.txt` | same | AGREE |
| d7s | 7 | yes | dodir + sibling file | `dirempty` stripped, file kept | same | AGREE |
| d8n | 8 | no | dodir + sibling file | `dirempty` stripped, file kept | same | AGREE |
| d8s | 8 | yes | dodir + sibling file | `dirempty` stripped, file kept | same | AGREE |

Merge stdout byte-identical modulo scratch path/ANSI on all 16 cells;
stderr empty on both sides everywhere. vdb CONTENTS identical modulo
mtime. `95empty-dirs`, `keepdir`, `dodir`, `eapi.sh` vendored copies
verified byte-identical to host Portage 3.0.82.2.

## Verdict: NO DIVERGENCE — "no divergence", change no product code

The S5 observation was expected Portage behaviour (`95empty-dirs`
strips bare `dodir` results on EAPI 8+ / under `strict-keepdir`;
`keepdir`'s `.keep` file is what preserves a directory). Both managers
agree on every cell. No failing test, no fix, no fixtures, no pins,
no corpus bless. Backlog entry #385 left for the owner to close
(not edited: brief forbids backlog/what-this-proves edits).

## Files changed

- `LLM/evidence/2026-10-19-385/` only (new, untracked): `SUMMARY.md`,
  `scripts/{setup,run,cells}.sh`, `cells/` (32 cells x
  argv/env/rc/stdout/stderr/image.find/image.empty/root.find/contents).
- Product code: none. pmtest: none.

## Residue / observations (no numbers filed, out of scope)

1. Post-merge `find $D` keeps merged regular files on portuale but not
   Portage (copy in `ebuild_merge/protect.rs::replace_file_atomic` vs
   Portage's rename-out `mergeme`). Deliberate portuale mechanics
   (image stays intact); ROOT and CONTENTS unaffected and identical.
   Only matters to future image-comparison harnesses: snapshot pre-merge
   or compare modulo regular files.
2. Fresh-ROOT side noise, uniform all cells: portuale-only
   `etc/ld.so.conf` (env-update header); Portage-only empty
   `var/lib/portage/preserved_libs_registry` + `var/tmp` (1777).
   Unrelated to #385; recorded, not pursued.
