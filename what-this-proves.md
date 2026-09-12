
**Tier-2 close-out B-series: a merged-order trace harness plus two real
fixes (#17, 2026-09-12).** `TEST/scripts/mo-trace/` is now the committed
tool that three sessions had prototyped and reverted: `real-trace.py`
injects one `RT_SEL` line per real `_serialize_tasks` iteration (plus a
one-time `MO_NODES` graph snapshot and an `MO_ORDER` post-prune/pre-bias
snapshot; idempotent, `--unpatch` byte-identical on 3.0.81.3 and
3.0.82.2), `PORTUALE_MO_SEL=1` emits the same `MO_SEL` shape from
`merge_order::select_nodes` (`iter/retlist/alive/asap/prefer_asap/
drop_satisfied/ig/pick`, picks marked `m:`/`n:`), and `align-traces.py`
reports the first divergence by iteration, by node set, or
`--merge-only` by merge sequence. Unset, portuale's output is
byte-identical (unit-pinned formatter).

```sh
# real side (throwaway container): patch, run, extract
podman run --rm -v "$PWD:$PWD:ro" -v "$PWD/rust/target/release:/usr/local/bin:ro" \
  --entrypoint /bin/bash localhost/test-portuale:latest -c '
    python3 '$PWD'/TEST/scripts/mo-trace/real-trace.py
    /usr/sbin/emerge --pretend --debug gui-libs/gtk:4 2>real.err >/dev/null
    grep -E "^(RT_SEL|MO_NODES|MO_ORDER) " real.err > real.trace'
# portuale side
TEST/scripts/mo-trace/ptl-trace.sh /tmp/ff -- --pretend gui-libs/gtk:4
python3 TEST/scripts/mo-trace/align-traces.py --merge-only real.trace /tmp/ff.portuale.trace
```

Two fixes landed from it. (1) `add_installed_dependency_closure` kept
one `InstalledPackage` per cp and keyed its dedup on `(category,
package)`, silently dropping every installed slot but the first --
gtk:4's real scheduler graph carried `docbook-xml-dtd-{4.2,4.4,4.5}`
that portuale lacked. It now keeps every version and selects the one an
edge's atom names: node sets 395 -> 398 == real, `texlive-core` and
`texlive-latex` flipped clean. (2) `build_digraph`'s forward-edge loop
edged a bare multi-slot atom (`llvm-runtimes/clang-runtime[...]` in
clang-common's PDEPEND) to every scheduled slot, giving
`clang-runtime-21` an extra `runtime_post` parent that promoted it into
`asap` and split the drain; the loop now resolves a multi-match atom to
one entry (merge-bound first, then highest version), moving
firefox/thunderbird's first divergence from #29/#30 to #37/#38. L0
(`TEST/logs/l0-20260912T205626Z`): clean 96 -> 98, parity 0.800 ->
0.817, order 22 -> 20 raw (19 after B5 explains gnome-shell's `order #0`
as the cluster-A abort bundle via `known-divergences.yaml`). The
residue is adjudicated, not chased: B1's frontier drain timing (578 vs
290 iterations on gtk:4), B3's `_create_graph` pre-bias insertion order,
and B4's superseded installed in-edges (both moved to #25) -- see
`docs/025-tier2-closeout.deepseek.md` §11.
