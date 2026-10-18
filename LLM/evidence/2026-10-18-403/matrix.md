# S403 matrix — merge/unmerge listing oracle (backlog #403, slice S10)

Evidence: this dir (`l/`). Scripts: `scripts/l.sh` (+ `lib.sh`).
Oracle: Portage 3.0.82.2 ("Portage"). portuale: `target/release/portuale`
at capture time prints **nothing** for the direct path (l01/l05 portuale
stdout is empty, rc 0) — the gap this slice closes.

## Cells

| cell | setup | Portage | portuale (today) | verdict |
|---|---|---|---|---|
| l01-ebuild-merge | `ebuild t348-b-phases-7 merge`, fresh ROOT | collision line + `>>> Merging …` + 8 `>>>` lines + `>>> … merged.` (full text §"l01") | empty stdout rc 0 | the fresh-merge pin |
| l02-emerge-new | `emerge --oneshot` new install | `>>> Installing/Completed (1 of 1)`, NO per-file lines, NO collision line, NO `Merging` line | same shape minus markers | emerge path stays silent (no-op gate) |
| l05-qmerge | merge 7, `install` 8, `qmerge` 8 | collision line + `>>> Merging` + `---`×4/`===`×2/`>>>`×2 + `>>> Safely unmerging …` + `--- replaced`×5 + `<<<`×2 + `>>> Original …` + `>>> … merged.` (full text §"l05") | empty stdout rc 0 | the replace pin; order is reverse-sorted CONTENTS |
| l06-symlink | synthetic `s403-link-1.0` (`payload.txt` + `rel-link -> payload.txt` + `abs-link -> /usr/…`), fresh merge | `>>> <p>/payload.txt`, `>>> <p>/rel-link -> payload.txt`, `>>> <p>/abs-link -> /usr/share/…` | empty stdout rc 0 | symlink shape pin |
| l07-cfgdivert | synthetic `s403-cfg`: merge 1.0, user-modify `/etc/s403-cfg.conf`, merge 2.0 | merge side `--- <root>/etc/` + `>>> <root>/etc/._cfg0000_s403-cfg.conf`; replace side `--- replaced obj …/s403-cfg.conf`, `--- replaced dir …/etc` | empty stdout rc 0 | diverted-write shape pin |
| l07b-cfgskip | as l07, then merge 2.0 content again | merge side `--- <root>/etc/s403-cfg.conf` (confmem rejects the already-offered update, no divert) | empty stdout rc 0 | skip shape pin |
| l08-collision | `s403-collide-1.0` owns `/usr/share/s403-owned.txt`; merge `s403-mod-1.0` shipping the same path; `protect-owned` on | collision line, then the collision report, then `Package … NOT merged due to file collisions`, rc 1 | rc 1 (no checking line yet) | abort parity already holds (rc); report text is NOT this slice |
| l08b-mergethrough | as l08 with `-protect-owned`: merge goes through | `merged despite file collisions` + plain `>>> <intruder>` (no `!owner` marker anywhere) | empty stdout rc 0 | kills the `--- !owner` phantom (no such marker in 3.0.82.2) |
| l09-cfgpro | `s403-mod-1.0` (ships `/lib/modules/s403-test/s403-test.ko`), then merge 2.0 which drops the `.ko` | old entry: `--- cfgpro   obj …/s403-test.ko`, `--- cfgpro   dir …/s403-test`, then `--- !empty   dir …/lib/modules`, `--- !empty   dir …/lib` | empty stdout rc 0 | `cfgpro` == UNINSTALL_IGNORE, NOT config-protect |
| l10-nocolor | l01 with `NOCOLOR=yes` | plain ` * checking …` (no ANSI) | empty stdout rc 0 (old binary) | color gate pin |
| l12-unmerge | `ebuild 8 merge`, then direct `ebuild 8 unmerge` | 3 `<<< obj` + 5 `<<< dir`, reverse-sorted, no frame lines | byte-identical (new binary) | direct-unmerge pin (no new markers) |

Ad-hoc (not script cells, same hermeticity):
`l-l02b-logdir-portage` (`PORT_LOGDIR` set): the persisted log ends after
the install phase — the listing is in neither it nor the terminal.
`l-l02c-noclean` (`FEATURES=noclean`): `${T}/build.log` persists but also
ends after install. So on the `emerge` path the listing reaches no
persistent sink at all (merge-child stdout discarded); the printer is a
no-op there and portuale's build log needs no change. `TERM=dumb` (manual
run): listing stays colored; `NO_COLOR=1`: plain — both follow
`_MergeProcess._target`'s `havecolor = not no_color(settings)`
(vartree.py:248), which consults neither tty nor TERM.

## l01 (fresh merge, verbatim modulo scratch root)

```
 <GOOD>*<reset> checking 3 files for package collisions
>>> Merging app-misc/t348-b-phases-7 to <ROOT>/
>>> <ROOT>/usr/
>>> <ROOT>/usr/share/
>>> <ROOT>/usr/share/t348-b-phases/
>>> <ROOT>/usr/share/t348-b-phases/built.txt
>>> <ROOT>/usr/share/t348-b-phases/config.args
>>> <ROOT>/usr/share/doc/
>>> <ROOT>/usr/share/doc/t348-b-phases-7/
>>> <ROOT>/usr/share/doc/t348-b-phases-7/README.md
>>> app-misc/t348-b-phases-7 merged.
```
(`<GOOD>` = `\x1b[32;01m`, `<reset>` = `\x1b[39;49;00m`.)

## l05 (qmerge over installed older, verbatim modulo scratch root)

```
 <GOOD>*<reset> checking 3 files for package collisions
>>> Merging app-misc/t348-b-phases-8 to <ROOT>/
--- <ROOT>/usr/
--- <ROOT>/usr/share/
--- <ROOT>/usr/share/t348-b-phases/
=== <ROOT>/usr/share/t348-b-phases/built.txt
=== <ROOT>/usr/share/t348-b-phases/config.args
--- <ROOT>/usr/share/doc/
>>> <ROOT>/usr/share/doc/t348-b-phases-8/
>>> <ROOT>/usr/share/doc/t348-b-phases-8/README.md
>>> Safely unmerging already-installed instance...
--- replaced obj <ROOT>/usr/share/t348-b-phases/config.args
--- replaced obj <ROOT>/usr/share/t348-b-phases/built.txt
--- replaced dir <ROOT>/usr/share/t348-b-phases
<<<          obj <ROOT>/usr/share/doc/t348-b-phases-7/README.md
--- replaced dir <ROOT>/usr/share/doc
--- replaced dir <ROOT>/usr/share
--- replaced dir <ROOT>/usr
<<<          dir <ROOT>/usr/share/doc/t348-b-phases-7
>>> Original instance of package unmerged safely.
>>> app-misc/t348-b-phases-8 merged.
```

## Marker inventory (Portage 3.0.82.2 `lib/portage/dbapi/vartree.py`)

All listing lines go to **stdout** (`writemsg_level(..., level=0)` → stdout
unless the merge runs as an emerge background task, which discards them —
see above). `!!!` lines go to stderr (`level=ERROR`) and are NOT this slice
(they ride the existing elog/eerror paths).

- ` <GOOD>*<reset> checking <N> files for package collisions` —
  `_collision_protect:4052-4055`. `N` = regular files + (non-dir)
  symlinks in the image (dirs excluded: l01 "3 files" for 3 files;
  l06 "3 files" for 1 file + 2 symlinks). Colorized unless `NO_COLOR` is
  set or `NOCOLOR=yes|true` (`bin/ebuild:128-134` gates the parent,
  `_MergeProcess._target:248` re-arms the merge child from settings only:
  piped output stays colored, `TERM=dumb` stays colored). Printed even
  when the merge then aborts (l08).
- `>>> Merging <cpv> to <destroot>/` — `treewalk:5049-5052`
  (`destroot` = ROOT with exactly one trailing slash). Before `preinst`,
  after the collision phase.
- Merge side, per entry in `mergeme:5489-5994` (DFS, pop-from-end of
  readdir-ordered lists — i.e. on this tmpfs the image creation order;
  §"Order" for why portuale keeps its own traversal):
  - symlink: `>>> <dest> -> <target>` when merged, `--- <dest> -> <target>`
    when config-memory rejects it (`:5654-5744`; `zing` starts `!!!`,
    becomes `---` at `:5650-5652` when `moveme` is false, `>>>` at `:5713`
    on move). Printed only when the target was reached (`mymtime` set);
    `mydest` is the `._cfgNNNN_` path when diverted (l07).
  - dir: `--- <dest>/` when the destination exists (or is a symlink to a
    dir, `:5805-5811`), `>>> <dest>/` when created (`:5859`, `:5878`).
  - regular file: `>>> <dest>` when `movefile` replaces (`_needs_move`,
    `:5916-5937`), `=== <dest>` when source and destination are identical
    (utime refresh only, `:5938-5945`), `--- <dest>` when confmem rejects
    an already-offered protected update (`_protect:6038-6042` → l07b).
    `mydest` is the `._cfgNNNN_` path when diverted (l07).
  - fifo/device: `>>> <dest>` when created, `!!! <dest>` when the
    destination already exists (`:5962-5994`; the `!!!` still rides stdout).
- `>>> Safely unmerging already-installed instance...` — `treewalk:5195`,
  once per replaced same-slot instance, before its unmerge.
- Unmerge side, `dblink.unmerge` → `_unmerge_pkgfiles:2869-3245`, keys in
  **reverse-sorted CONTENTS order** (`mykeys.sort(); mykeys.reverse()`),
  `--- replaced` inline, `<<<` inline, dirs deferred to `_unmerge_dirs`:
  - `--- <desc8> <type> <path>` / `<<<␣␣␣␣␣␣␣␣<type> <path>` —
    `_show_unmerge:2866-2867` (`f"{zing} {desc.ljust(8)} {file_type} {file_name}"`;
    `<<<` carries an empty desc, hence 10 spaces before the type).
  - `--- replaced` — path claimed by another same-slot instance (`:3090`).
  - `--- cfgpro` — path matches `UNINSTALL_IGNORE` (`:3047-3048`;
    e.g. `/lib/modules/*`) or is a protected libdir symlink (`:3043`).
    **Not** config-protect: `/etc` files divert on the merge side instead.
  - `<<<` — file actually removed (orphan arm `:3111`,
    symlink `:3194`, obj `:3223`).
  - `--- !mtime` (`:3118`), `--- !dir` (`:3123`), `--- !sym` (`:3128`),
    `--- !found` (lstat fails, `:3032`), `--- !md5` (`:3215`),
    `--- !obj` (`:3202,3209`), `--- !fif` (`:3226`), `--- !prefix`
    (`:3017`), `---` + empty desc for a kept live fifo (`:3228`) or a kept
    `dev` (`:3230`).
  - `_unmerge_dirs:3370-3493`: dirs reverse-sorted, `os.rmdir`;
    `<<< dir <path>` on success (`:3441`), `--- !empty dir <path>` when it
    is still non-empty (`:3446`), info-dir leftovers `<<< obj` (`:3407`).
    Hence l05's tail: the seven `--- replaced`/`<<<` lines run in the main
    loop's reverse-sorted order, then the one surviving dir
    (`doc/t348-b-phases-7`) is removed by `_unmerge_dirs` last.
  - There is **no `!owner` marker** in 3.0.82.2 (verified by tree-wide
    grep): a file owned by another package with neither `collision-protect`
    nor `protect-owned` merges straight through with a plain `>>>` (l08b).
- `>>> Original instance of package unmerged safely.` — `treewalk:5222`,
  after each replaced instance's vdb entry is deleted.
- `>>> <cpv> merged.` — `treewalk:5348`, after a successful `postinst`
  (skipped when postinst fails).

## Order

- Replace/unmerge side: byte-reproducible — reverse-sorted CONTENTS keys,
  inline `--- replaced`, deferred `_unmerge_dirs`. portuale's existing
  traversal (`remove_contents` reverse-sorted + `remove_dirs` LIFO) already
  matches; the slice only adds the prints.
- Merge side: Portage's order is readdir-derived (pop-from-end; on tmpfs ==
  image creation order — l01/l05/l06 all stable across the S0 and S403
  runs). portuale deliberately traverses sorted (`mod.rs:16`, determinism
  over `os.listdir()` order). No sorted traversal can reproduce a
  readdir-derived order (l01 needs reverse-sorted siblings at one level
  and sorted at the next), so portuale prints inline in its own traversal
  order: same line SET as Portage (verified per cell), sibling-subtree
  order differs by design. The pmtest pins compare the entry lines as a
  root-normalized multiset plus verbatim anchors (collision/`Merging`/
  `Safely`/`Original`/`merged.` lines, full replace-side block);
  portuale's own output stays byte-stable run to run.

## Deliberate non-goals (for the report)

- Collision/error report text (`eerror` blocks, `!!!`, `bak`) — existing paths.
- Protected-symlink R4b search elog — existing `unmerge_protected_symlinks`.
- `emerge`-path output — byte-identical by construction (printer no-op).
- `PORTAGE_VERBOSE=1` direct-path extra text — `_display_merge` ignores
  `_verbose` once a scheduler exists (both paths), so no extra text exists.
- `ebuild unmerge` (direct standalone) prints the same `_unmerge_pkgfiles`
  listing on Portage; portuale prints it too (same direct-path gate, S403
  l12 byte-identical). `emerge -C` prints none on either side (l04 in S0).

## portuale-vs-Portage stdout diff (root-normalized, new binary)

- l07, l07b, l09, l12: **identical**.
- l01, l05, l06, l10: differ in 4 lines only — the merge-side
  sibling-subtree order (Portage readdir-derived, portuale sorted;
  §"Order"). Every line present on both sides.
- l08b: Portage's 24-line collision report (`eerror` block, pre-existing
  portuale gap — the abort/message text is not this slice) plus the same
  sibling-order delta; all listing lines present on both sides.
- l02/l04: no listing lines on either side (l02's `>>> Installing /
  Completed` lines match on both).
