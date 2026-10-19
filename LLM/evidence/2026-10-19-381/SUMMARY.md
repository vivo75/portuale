# S0 SUMMARY — backlog #381, oracle first

Oracle: Portage 3.0.82.2 (host `emerge`). portuale at `9a909749`
(pre-fix binary for the `-pl` runs; rebuilt with the fix for the
post-fix `-pl` reruns — same rev, see `cells/*-pl-2.env` for mtimes).
No sudo used. No product code, tests, backlog or plan files touched by
the S0 itself (the fix and pins came after).

Harness: `scripts/setup.sh` (scratch overlay `app-misc/t381-rep`,
`t381-k` + hermetic configroot under `/var/tmp/pt-381-s0`, same shape
as the #355 S0), `scripts/run.sh` (per-PM hermetic roots,
`cells/<tag>.{argv,env,rc,stdout,stderr}`), `scripts/cells.sh`
(V1/V2/V3 for `T=pg|pl`). No `sudo` used.

Fixture shapes (all shell-only ebuilds, no gcc needed):

- `t381-rep-1`/`-2` (same PN+SLOT, `T381_W` switch like `T355_V`):
  `normal` records `obj /usr/share/t381/f`, `alias` records
  `obj /alias/f`. A live `/alias -> usr/share/t381` symlink is created
  between the merges. The second merge's replace loop unmerges the
  first instance with `others_in_slot=[new]`.
- `t381-k-1` (`keepdir /usr/share/t381/sub`), `t381-k-2`
  (`insinto /alias/sub`, `obj /alias/sub/f`).

## Per-cell verdicts

- **V1** (old=normal, new=alias): Portage `--- replaced obj
  …/usr/share/t381/f`, file survives (`V1-pg-2.stdout`,
  `V1-pg.tree`). Pre-fix portuale deleted the file (`V1-pl.tree`
  before the fix had no `t381/f`; vdb claimed dangling `/alias/f`).
  Post-fix portuale keeps it: `V1-pg.tree` vs `V1-pl.tree` identical
  modulo env noise (`etc/ld.so.conf`, `tmp/`, `mtimedb`,
  `preserved_libs_registry`) and merge timestamps in CONTENTS.
- **V2** (old=alias, new=normal): Portage `--- replaced obj …/alias/f`
  + `<<< dir …/alias`, file survives. Pre-fix portuale deleted it;
  post-fix identical to Portage modulo the same noise.
- **V3** (old=keepdir sub, new=alias/sub/f): Portage `<<< obj
  …/.keep_app-misc_t381-k-0` then `--- replaced dir
  …/usr/share/t381/sub`; tree agrees pre- and post-fix (sub + f
  survive on both). Listing-only divergence, not visible in `emerge`
  output (no per-file listing at default verbosity).

The `--- replaced` lines are Portage's `is_owned` arm
(`vartree.py:3054-3061` → `dblink.isowner` → `_match_contents`
`:3527-3613`): the queried parent's live `(dev, ino)` (stat follows
the symlink) matches a recorded parent's, and the basename rebuilt
under that recorded parent is recorded. portuale's same-slot check was
exact-string only (`contents_text_owns`).

## Analysis notes (for the implementer)

- The inode form needs only *explicit* recorded parents: with real
  merges every ancestor of a recorded path is recorded explicitly, so
  the implicit-parent exact arm and the basename shortcut are
  verdict-neutral (no cell can reach them).
- The cross-package R4b search (`contents_owns_or_under`,
  exact-or-prefix) provably cannot diverge with live records: a
  second-spelling same-basename record with a same-inode parent is the
  same live dentry, i.e. either the same spelling or a merge
  collision. Left unchanged, documented in the code note.
- No hardlink-file cell: `_match_contents` never compares file inodes,
  so a same-inode hardlink elsewhere is ownership-neutral on both
  sides (nothing observable to pin).
