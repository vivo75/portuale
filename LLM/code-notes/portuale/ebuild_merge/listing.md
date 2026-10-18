# crates/portuale/src/ebuild_merge/listing.rs
One listing printer for the direct-path merge and unmerge (backlog #403):
every line Portage's `dblink` prints on the foreground path, on stdout.

## struct MergeListing
- Ports `dblink._display_merge` (vartree.py:2841-2864) on the foreground
  path only: when a scheduler exists the `_verbose` gate is bypassed and
  the line goes to `scheduler.output` (stdout when foreground, build log
  when background). The printer is therefore a no-op for the emerge path
  (`MergeOptions.direct == false`): on Portage's side that listing reaches
  neither the terminal nor any persistent log (S403 l02/POR_LOGDIR/noclean
  probes in `LLM/evidence/2026-10-18-403/matrix.md`), so there is nothing
  to reproduce in portuale's build log.
- `emit` records every line (unit tests assert on `lines()`) and prints
  only when enabled; a disabled listing is a complete no-op.
- Color follows `_MergeProcess._target` (vartree.py:248,
  `havecolor = not no_color(settings)`): always on except `NO_COLOR` set
  or `NOCOLOR=yes|true`. Piped output stays colored and `TERM=dumb` stays
  colored, unlike the pretend renderer (S403 l10 + manual `TERM=dumb` /
  `NO_COLOR=1` probes). `new_for_test` pins the switch per test because
  the process environment is global.

## marker inventory
- `collision_line` ports `_collision_protect` (vartree.py:4052-4055):
  `N` is regular files plus non-directory symlinks, directories excluded.
  Printed before the abort check, so it appears even when the merge then
  aborts (S403 l08).
- `merging`/`merged` port `treewalk` (vartree.py:5049-5052, :5348):
  `>>> Merging <cpv> to <root>/` (exactly one trailing slash) before
  `preinst`, `>>> <cpv> merged.` only after a successful `postinst`.
- `merge_dir`/`merge_file`/`merge_symlink`/`merge_special` port `mergeme`
  (vartree.py:5654-5994): `>>>` new/diverted, `---` kept or confmem-
  rejected, `===` byte-identical (`_needs_move`, :6363-6393), `->` target
  suffix for symlinks, `!!!` for an already-existing fifo/device (still
  stdout, level 0).
- `show_unmerge` ports `_show_unmerge` (vartree.py:2866-2867):
  `f"{zing} {desc.ljust(8)} {file_type} {file_name}"` — `<<<` carries an
  empty desc (10 spaces before the type).
- `safely_unmerging`/`original_unmerged` port `treewalk` (:5195, :5222),
  once per replaced same-slot instance.

## order
- Merge side: Portage's order is readdir-derived (pop-from-end; on tmpfs
  the image creation order, stable across the S0 and S403 runs).
  portuale keeps its sorted traversal (`mod.rs` determinism rule — no
  sorted traversal can reproduce a readdir-derived order) and prints
  inline, so the line SET matches Portage while sibling-subtree order may
  differ by design. The pmtest pins compare the entry lines as a
  root-normalized multiset with verbatim anchors.
- Unmerge side: byte-reproducible — reverse-sorted CONTENTS keys with
  inline `--- replaced` and deferred `_unmerge_dirs` (S403 l05 ≡ portuale).

## brief corrections (oracle beats the brief)
- There is no `!owner` marker in 3.0.82.2 (tree-wide grep): a file owned
  by another package with neither `collision-protect` nor `protect-owned`
  merges straight through with a plain `>>>` (S403 l08b).
- `--- cfgpro` is the `UNINSTALL_IGNORE` match (vartree.py:3047-3048),
  not CONFIG_PROTECT: `/etc` files divert on the merge side instead.
