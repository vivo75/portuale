"""R6 S0: per-pass trace of Portage's _backtrack_depgraph counter (read-only overlay).
Re-execs an instrumented copy of _emerge.depgraph._backtrack_depgraph (source text taken
from the installed module, only trace statements inserted; assertions check every anchor
matched exactly once) and wraps Backtracker._add/get/feedback/get_best_run call-through.
Trace lines go to stderr prefixed 'R6 '."""
import sys, inspect, textwrap
from _emerge import depgraph as dg
from _emerge.resolver import backtracking as bt

def W(s):
    sys.stderr.write("R6 " + s + "\n"); sys.stderr.flush()
dg._r6W = W

def P(pkg):
    try:
        return f"{pkg.cpv}({pkg.type_name}{',' + pkg.operation if pkg.operation != 'merge' else ''})"
    except Exception:
        return str(pkg)
def M(mask):
    return "{" + "; ".join(f"{P(p)}:{'+'.join(sorted(info))}" for p, info in mask.items()) + "}"
dg._r6M = M

src = textwrap.dedent(inspect.getsource(dg._backtrack_depgraph))
def sub(old, new):
    global src
    assert src.count(old) == 1, (src.count(old), old)
    src = src.replace(old, new)

sub("""        success, favorites = mydepgraph.select_files(myfiles)

        if (""", """        success, favorites = mydepgraph.select_files(myfiles)
        _dc = mydepgraph._dynamic_config
        _r6W(f"PASS bt_before={backtracked} mask={_r6M(backtrack_parameters.runtime_pkg_mask)} "
             f"success={bool(success)} need_restart={mydepgraph.need_restart()} "
             f"(_need_restart={_dc._need_restart} _skip_restart={_dc._skip_restart}) "
             f"infos={sorted(mydepgraph.get_backtrack_infos())} unexplored_after_get={len(backtracker)}")
        if (""")
sub("""            or backtracked >= max_retries
        ):
            break""", """            or backtracked >= max_retries
        ):
            _r6W(f"  ARM break: success={bool(success)} need_config_change={None if success else mydepgraph.need_config_change()} "
                 f"allow_backtracking={allow_backtracking} limit={backtracked >= max_retries} -> backtracked stays {backtracked}")
            break""")
sub("""            backtracked += 1
            backtracker.feedback(mydepgraph.get_backtrack_infos())""", """            backtracked += 1
            backtracker.feedback(mydepgraph.get_backtrack_infos())
            _r6W(f"  ARM restart D:12414: backtracked -> {backtracked}; unexplored after feedback={len(backtracker)}")""")
sub("""        elif backtracker:
            backtracked += 1""", """        elif backtracker:
            backtracked += 1
            _r6W(f"  ARM failed-no-restart, unexplored nonempty D:12417: backtracked -> {backtracked}")
        else:
            _r6W(f"  ARM failed-no-restart, unexplored EMPTY: backtracked stays {backtracked}; loop ends")""")
sub("""    if backtracked and not success and not mydepgraph.need_display_problems():""",
    """    _r6W(f"LOOP-END backtracked={backtracked} success={bool(success)} need_display_problems={mydepgraph.need_display_problems() if (backtracked and not success) else 'n/a'}")
    if backtracked and not success and not mydepgraph.need_display_problems():
        _r6W("BEST-RUN pass D:12420-12437 (no increment)")""")
sub("""    if not success and mydepgraph.autounmask_breakage_detected():""",
    """    if not success and mydepgraph.autounmask_breakage_detected():
        _r6W("BREAKAGE clean pass D:12439 (no increment)")""")
sub("""    return (success, mydepgraph, favorites, backtracked, max_retries)""",
    """    _r6W(f"RETURN backtracked={backtracked}/{max_retries} success={bool(success)}")
    return (success, mydepgraph, favorites, backtracked, max_retries)""")
code = compile(src, dg.__file__ + ":_backtrack_depgraph[r6]", "exec")
exec(code, dg.__dict__)

o_add = bt.Backtracker._add
def add(self, node, explore=True):
    n, u = len(self._nodes), len(self._unexplored_nodes)
    valid = self._check_runtime_pkg_mask(node.parameter.runtime_pkg_mask)
    depth_ok = node.mask_steps <= self._max_depth
    dup = node in self._nodes
    o_add(self, node, explore=explore)
    acc = len(self._nodes) > n
    why = "" if acc else ("rpm-invalid(bug375573)" if not valid else "mask_steps>max_depth" if not depth_ok else "duplicate")
    W(f"    _add mask_steps={node.mask_steps}/{self._max_depth} explore={explore} "
      f"{'ACCEPTED' if acc else 'REJECTED ' + why} mask={M(node.parameter.runtime_pkg_mask)}")
bt.Backtracker._add = add

o_fb = bt.Backtracker.feedback
def fb(self, infos):
    W(f"    feedback keys={sorted(infos)}")
    return o_fb(self, infos)
bt.Backtracker.feedback = fb

from _emerge.main import emerge_main
sys.exit(emerge_main(sys.argv[1:]))
