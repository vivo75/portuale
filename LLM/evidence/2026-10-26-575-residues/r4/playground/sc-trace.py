"""R4: log what Portage's slot_conflict_handler does, without changing it.
Wraps (call-through only): depgraph.need_config_change, depgraph._show_slot_collision_notice,
depgraph._backtrack_depgraph. Writes JSON lines to $SC_TRACE_LOG (stdout/stderr untouched).
Then runs the installed emerge script with runpy.
Usage: SC_TRACE_LOG=<file> python3 -I sc-trace.py <emerge args...>"""
import json, os, runpy, sys
import _emerge.depgraph as D

LOG = os.environ["SC_TRACE_LOG"]
def W(**k):
    with open(LOG, "a") as f:
        f.write(json.dumps(k, default=str) + "\n")

def hinfo(h):
    if h is None:
        return None
    return dict(
        conflicts=[(str(a), [f"{p.cpv}:{p.type_name}{'' if p.operation == 'merge' else ',' + p.operation}" for p in pk]) for r, a, pk in h.all_conflicts],
        is_a_version_conflict=h.is_a_version_conflict,
        conflict_is_unspecific=h.conflict_is_unspecific,
        n_solutions=len(h.solutions),
        changes=[{f"{p.cpv}:{p.type_name}": v for p, v in c.items()} for c in h.changes],
        explanation=h.get_explanation(),
    )

o_ncc = D.depgraph.need_config_change
def ncc(self):
    dc = self._dynamic_config
    was_none = dc._slot_conflict_handler is None
    r = o_ncc(self)
    h = dc._slot_conflict_handler
    W(ev="need_config_change", ret=r, allow_backtracking=dc._allow_backtracking,
      built_here=was_none and h is not None,
      slot_arm_stop=bool(was_none and h is not None and h.changes),
      success_without_autounmask=dc._success_without_autounmask,
      required_use_unsatisfied=dc._required_use_unsatisfied,
      autounmask_backtrack_disabled=getattr(dc, "_autounmask_backtrack_disabled", None),
      handler=hinfo(h) if (was_none and h is not None) else None)
    return r
D.depgraph.need_config_change = ncc

o_show = D.depgraph._show_slot_collision_notice
def show(self):
    dc = self._dynamic_config
    pre = dc._slot_conflict_handler is not None
    r = o_show(self)
    W(ev="show_slot_collision_notice", handler_reused=pre, allow_backtracking=dc._allow_backtracking,
      handler=hinfo(dc._slot_conflict_handler))
    return r
D.depgraph._show_slot_collision_notice = show

o_bt = D._backtrack_depgraph
def bt(*a, **k):
    r = o_bt(*a, **k)
    W(ev="backtrack_depgraph", success=r[0], backtracked=r[3], max_retries=r[4],
      final_allow_backtracking=r[1]._dynamic_config._allow_backtracking)
    return r
D._backtrack_depgraph = bt

sys.argv = ["emerge"] + sys.argv[1:]
runpy.run_path("/usr/lib/python-exec/python3.14/emerge", run_name="__main__")
