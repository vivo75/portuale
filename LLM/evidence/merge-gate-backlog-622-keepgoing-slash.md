# Merge-path safety gate: branch backlog/622-keepgoing-slash (#622)

Recipe: binary frozen from this branch's tree
(`cp -p target/release/portuale /var/tmp/bedbin/portuale`, `cmp`-verified,
from commit `fab9bc9c` rebased onto main at `1253e23b`), then from pmtest
(on branch `backlog/622-keepgoing-slash`, commit `b6cc422`):

```
PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1 \
  GENTOO_MIRRORS=http://10.48.0.229:8080 \
  differential-test-bed/run/l1-merge-from-binpkg.sh \
  differential-test-bed/atomlists/l1-merge-gate.txt
L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1 \
  PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1 \
  GENTOO_MIRRORS=http://10.48.0.229:8080 \
  differential-test-bed/run/l1-merge-from-binpkg.sh \
  differential-test-bed/atomlists/l1-merge-gate.txt
```

Container note (host-state, not part of the slice): `podman run` as vivo
failed with crun's `sd-bus ... interactive authentication` denial, so the
`localhost/test-portuale:latest` image was rebuilt with
`XDG_CONFIG_HOME` pointed at a throwaway `containers.conf` carrying
`cgroup_manager="cgroupfs"` (verified with plain `podman run` and
`buildah run` first); `create-container.bash` itself is unmodified. Both
gate passes and the image build below ran with that override in the
environment only.

## Pass 1 — build + merge (`l1-20261101T193951Z`)

- runner rc 0
- `sys-libs/glibc` (2.43-r4) and `app-shells/bash` (5.3_p20) merged by both
  PMs: `merged_count 2` on both sides, `merge_rc 0` both sides
- hard findings 0, UNEXPLAINED 0, explained 0, payload diffs 0
- mtime-only diffs 1420 (non-fatal; same count as the #574 gate run)

## Pass 2 — reinstall over the live set (`l1-20261101T201312Z`)

`L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1` forces both PMs to re-merge the
already-installed pair over their own running container (the gate's real
shape: portuale's shell children run from the very `/bin/bash` it
replaces, every process maps the libc being replaced).

- runner rc 0
- `sys-libs/glibc` (2.43-r4) and `app-shells/bash` (5.3_p20) re-merged by
  both PMs: `merged_count 2` on both sides, `merge_rc 0` both sides
- hard findings 0, UNEXPLAINED 0, explained 0, payload diffs 0
- mtime-only diffs 1420 (non-fatal)

Logs are local-only under pmtest
`differential-test-bed/logs/l1-20261101T193951Z/` and
`differential-test-bed/logs/l1-20261101T201312Z/`.
Verdict: PASSED.

The third (`INSTALL_MASK`) invocation is not run: it is required for
changes to the unmerge half of the merge path, and this slice touches
neither merge nor unmerge logic (one display `format!` string in
`Ledger::dropped`).
