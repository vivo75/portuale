# Merge-order trace harness (`#17`)

The tool that makes the cluster-I (`_serialize_tasks` frontier timing)
investigation tractable. Three sessions prototyped and reverted it
before; it lives here now so nobody re-derives it (025 plan B0).

## What it does

Both schedulers run a repeated "pick the currently-eligible leaves"
loop. When the merge *list* differs, the interesting question is at
**which iteration** the two frontiers first disagree -- the merge-list
diff only shows the downstream symptom. This harness dumps one state
line per iteration from each side and aligns them.

Real side (`RT_SEL`): `real-trace.py` injects a block into the
container's `_emerge/depgraph.py::_serialize_tasks`, right before it
removes the selected nodes.

Portuale side (`MO_SEL`): `PORTUALE_MO_SEL=1` makes
`rust/portage-repo/src/merge_order.rs::select_nodes` print the same line
format. Unset, output is byte-identical to a normal run (pinned by the
`mo_sel_trace_line_pins_the_harness_format` unit test).

Format (field order is the harness contract):

```
MO_SEL iter=7 retlist=3 alive=42 asap=1 prefer_asap=0 drop_satisfied=1
       ig=ignore_runtime pick=dev-libs/a-1 dev-libs/b-2
```

- `retlist` -- merge packages selected so far (real appends only merges)
- `alive` -- non-uninstall packages still in the graph (post-prune)
- `asap` -- real's `asap_nodes` length
- `ig` -- `ignore_priority.__name__` (real) / the same name ported
- `pick` -- the nodes selected this iteration, `m:cat/pkg-ver` for a
  merge-bound node, `n:cat/pkg-ver` for an installed nomerge one (the
  `m`/`n` prefix is what makes a "real drained installed leaves while
  portuale drained merge leaves" divergence visible at iteration 1)

## Local (portuale-only) use

```sh
TEST/scripts/mo-trace/ptl-trace.sh /tmp/gtk4 -- --pretend --debug app-misc/gtk:4
# -> /tmp/gtk4.portuale.trace
```

## Full real-vs-portuale run (container)

```sh
# 1. patch real inside the throwaway container, run real emerge, keep stderr
sudo podman run --rm -v "$PWD:$PWD:ro" -v "$PWD/rust/target/release:/usr/local/bin:ro" \
    --entrypoint /bin/bash localhost/test-portuale:latest -c "
        python3 $PWD/TEST/scripts/mo-trace/real-trace.py &&
        /usr/sbin/emerge --pretend app-misc/gtk:4 --debug >/tmp/real.out 2>/tmp/real.err;
        grep '^RT_SEL ' /tmp/real.err > $PWD/gtk4.real.trace; cat /tmp/real.out"

# 2. portuale side on the same tree is the L0 probe's own output; locally
#    against fixtures the tree differs, so real-tree comparisons come from
#    the container.
TEST/scripts/mo-trace/ptl-trace.sh /tmp/gtk4 -- --pretend app-misc/gtk:4

# 3. align
python3 TEST/scripts/mo-trace/align-traces.py gtk4.real.trace /tmp/gtk4.portuale.trace
```

The aligner reports the first iteration whose `alive`/`asap`/`ig`/`pick`
fields differ -- that iteration is where to look (the newest node whose
leaf state differs, and the edge/priority that keeps it alive on one side
but not the other).

## Notes

- Patch only a throwaway container (or a copy); `--unpatch` restores the
  file byte-for-byte.
- Real's container is PID-1 `/init`-based; the `podman run --entrypoint
  /bin/bash` form above bypasses it so the patch and the emerge run are
  one shell.
- The Python reference (`python/emerge_pretend_reference.py`) does not
  mirror the trace: complete-mode real-tree graphs take minutes in
  Python, so the harness is Rust-only by design.
