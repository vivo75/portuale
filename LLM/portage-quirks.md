# Portage quirks and suspected bugs

Portuale mimics Portage (3.0.82.2) closely, because Portage is the oracle. But
knowing where Portage itself looks wrong has value: for users who hit the same
surprise, for upstream reports, and for deciding later whether to copy a quirk
or fix it. Every place where a Portage behaviour looks unintended gets an entry
here, whether or not portuale copies it.

**Important: which applet follows Portage.** `portuale emerge` is bound to
Portage's behaviour strictly: it copies every quirk below, because its output
must match Portage's. `portuale mrg` (the Mrg interface) is free to differ
where Portage is suspected to be buggy: an entry here is where that
divergence would be decided. Until a `mrg` divergence is implemented, the entry
says "copied by `emerge`" and nothing more.

Each entry says: what Portage does (with source citation and how it was
observed), the **first Portage version where it was seen**, why it looks wrong,
whether `portuale emerge` / `portuale mrg` copy it, and a status (`suspect` =
looks wrong, not yet confirmed with upstream; `confirmed` = an upstream bug
exists; `by design` = intended but surprising). The first-seen version is the
oldest Portage the behaviour was observed on; when only the current one was
checked, say so (this is not a claim that older versions are free of it).

Add new entries at the end. Do not rewrite old ones; correct them in place only
when a claim turns out false.

## Q1. `emerge -uD` silently skips an update when an installed node is already in the graph (suspect)

- **Observed** (2026-10-22, host `@world`, backlog #484; re-confirmed on the
  repaired binhost cache, see #487):
  `sys-firmware/edk2-bin` 202408 is installed; 202608 is visible and newer. The
  dep `|| ( sys-firmware/edk2 sys-firmware/edk2-bin )` of an installed parent
  resolves to the installed 202408, and no update is offered, and no "skipped
  due to a dependency conflict" notice is printed.
- **Why**: `_wrapped_select_pkg_highest_available_imp` first looks for an
  existing node in the graph (`find_existing_node`, `depgraph.py` ~8190). The
  package tracker's `match(..., installed=False)` still lists installed
  packages that earlier deps added as graph nodes, so an installed node pinned
  earlier by a different parent (here the `qemu` binary's baked
  `~sys-firmware/edk2-bin-202408`) satisfies the later bare atom, and the newer
  version is never considered.
- **Why it looks wrong**: the result depends on the order in which deps are
  walked. Had the bare atom been walked first, Portage would have picked 202608,
  hit a slot conflict with `qemu`'s pin, and printed the missed-update notice.
  The user sees an update vanish silently in one order and a loud warning in the
  other.
- **First seen**: Portage 3.0.82.2 (the only version checked; the
  `find_existing_node` pass is old, so earlier versions very likely behave the
  same).
- **Portuale**: `portuale emerge` copies it (`graph_installed_reuse` in
  `pass_helpers.rs`), because the merge list must match. `portuale mrg`: not
  yet decided. Fixture: pmtest `f484b-*`.

## Q2. The missed-update notice depends on binhost build timestamps (suspect)

- **Observed** (2026-10-22, host `@world`, backlog #484; re-confirmed on the
  repaired binhost cache, see #487): `app-misc/mime-types`
  has an installed instance (build 2, `USE=nginx`) and a binhost instance
  (build 1, `USE=-nginx`, newer `BUILD_TIME`). `--newuse` makes the first walk
  pick the binhost build; `www-servers/nginx` needs `[nginx]`; the slot conflict
  removes the binary and keeps the installed one. Portage then prints
  "WARNING: One or more updates/rebuilds have been skipped due to a dependency
  conflict" for `mime-types`.
- **Why**: `_get_missed_updates` skips the notice only if the kept package is
  greater than the removed one, and for two built packages of the same version
  `Package.__gt__` compares `BUILD_TIME` (`Package.py` 850-858). The binhost
  build is newer, so the kept installed package counts as "older".
- **Why it looks wrong**: the "skipped update" is the same version and the same
  cpv; it is a rebuild with different USE, and the reported conflict is just
  the USE the user already runs. Had the installed build been newer by
  timestamp, there would be no notice. A timestamp is an accident of when a
  binhost was filled.
- **First seen**: Portage 3.0.82.2 (the only version checked; the `BUILD_TIME`
  tie-break in `Package.__gt__` belongs to the binpkg-multi-instance work, so
  earlier versions without it do not have this behaviour).
- **Portuale**: `portuale emerge` copies it (`installed_twin_is_newer_build` in
  `direct_solve.rs`; the notice names `binary` and the build id). `portuale
  mrg`: not yet decided. Fixture: pmtest `f484-*` (both timestamp orders).
