# Owner question — plan 02.575 Task 11 (global updates wired into emerge)

Status: answered 2026-10-26 (rulings at the end). Asked 2026-10-26 by the manager, overnight run.

## 1. L1 bed run with an updates fixture (plan Task 11, last bullet)

The plan asks for an L1 bed run "with an updates fixture if the bed
supports one (else owner gate)". The bed has no such fixture: no atom
list or bed image carries a `profiles/updates` file with unrecorded moves
over installed packages. Task 11 was checked instead by:

- the Rust tests (fresh root applies and records, repeat run silent,
  `--package-moves=n` and info/sync/version skip);
- a host Portage 3.0.83 vs portuale probe on identical fixture copies
  (`emerge -p dev-libs/newmovepkg`): same stdout block, same vdb moves,
  same `mtimedb["updates"]` key and mtime;
- the merge gate (glibc + bash), whose bed roots carry the Gentoo tree's
  own updates record.

Question: add a bed cell (a container root whose record is stale, so both
Portage and portuale apply moves before a merge), or accept the checks
above?

## 2. Shape of a freshly created mtimedb

Portage writes the full `_MTIMEDBKEYS` set (`version`, `starttime`,
`ldpath`, `info`, …) into a new `mtimedb`. portuale writes only the
sections it manages and keeps any other keys untouched, which was the
Task 1 design. Both work; the bytes differ. Keep it as it is, or file an
item to write Portage's full shape?

## 3. Running the suite as root

As root, the running root `/` is writable, so a suite run with
`PORTAGE_RUNNING_ROOT=/` would apply the host's unrecorded updates and
write `/var/cache/edb/mtimedb`, as Portage would. The suite runs as
`vivo` here, so nothing happens. A CI that runs it as root would need a
scratch running root. Note only, no change made.

## Owner rulings (2026-10-26)

1. The checks above are enough; no bed cell.
2. File an item to write Portage's full shape: filed as #585.
3. A guard is wanted; the manager proposes options and the owner picks.
4. (Task 20 question) The `G0`/`G1`/`G2` labels in
   `reverse_dep_pins.rs` comments stay.
