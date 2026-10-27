# T7 S0: the shape that shows the system-runtime-deps gate, and the bed cell for it

Reference Portage is 3.0.83 (`3rdparty/portage`). All citations are
`lib/_emerge/Scheduler.py` unless noted.

## Why only a cycle shows it

`_choose_pkg` never starts a package whose deep dependencies are still
scheduled (`_dependent_on_scheduled_merges`, `:2071-2115`), so a system
package S normally merges *after* its runtime child X, and the set stays
empty. The set fills only when the merge order puts S first anyway, i.e. a
dependency cycle, which is exactly the case the docstring names
(`_system_merge_started`, `:1686-1693`: "circular dependencies"; bugs
#256616, #259954).

Concretely: S carries `PDEPEND` on X, X carries `RDEPEND` on S. Merge order
breaks the cycle S-first (post-merge edges do not gate the order). When S's
merge starts, the start-listener (`:1837-1838`, registered at `_build_exit`
when the finished build is queued) adds X — direct child, unsatisfied
`runtime_post`, operation `merge`, not completed — to
`_unsatisfied_system_deps` (`:1719-1730`). While the set is non-empty *and*
jobs run, `_schedule_tasks_imp` starts no new job (`:2469-2476`). An
unrelated package U that becomes ready while S merges therefore does not
start until X lands; the drain is `_task_complete` (`:1870-1872`), and the
set is cleared in `_main_loop_cleanup` (`:1988`). A system package is queued
even with `FEATURES=-merge-wait` (`:1833-1835`), so the listener still
fires there. The whole registration is skipped when `ROOT != "/"`.

## Bed cell spec (L3, source parity; run deferred to the manager)

- Container, `ROOT=/`, `--jobs 2`.
- Overlay with two ebuilds: `sys-libs/s-1` (`PDEPEND="dev-libs/x"`) and
  `dev-libs/x-1` (`RDEPEND="sys-libs/s"`), plus an unrelated `dev-libs/u-1`.
  S must be an `@system` member on the Portage side (profile `@system`
  entry in the bed container) and in portuale's `system_packages`.
- `emerge -v sys-libs/s dev-libs/x dev-libs/u` on both sides.
- Evidence: Portage's `emerge.log` timestamps — S's `>>> emerge ...`
  start, X's `::: completed` land, U's start. Expected on the Portage
  side: U starts only after X lands (with S's merge still in flight at
  U-ready time, so the `:2472` gate bites rather than the merge-wait
  queue drain). portuale must show the same order.
- No bed run was done in this slice: the container run (frozen binary,
  `l3-source-parity.sh`) is the manager's gate. The port itself is
  grounded in the source above plus the scheduler unit tests, which drive
  the identical cycle shape (`schedsys`/`schedwait`/`schedok` wired as
  S/X/U in `tests/scheduler.rs`).

## Why no host probe

The gate is skipped unless `ROOT == "/"` (`:1699-1702`), so a scratch-root
host probe cannot show it, and merging at `/` on this host is out of the
question. The unit tests cover the `/` path at the helper level with
fixture-named entries (never installed on any host, so the
installed-satisfaction check is deterministically false) and the off-`/`
path through the full scheduler.
