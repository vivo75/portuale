# STOP-1 (runbook 1, Task 1, Step 6): the #766 roots moved

## Rule

Task 1, Step 6 of `LLM/02.768-769-runbook.md`: "One `DIFFERENT` line is **STOP-1**."
Expected: eleven `same` lines for the roots ctl prov r698 r703 bdw pull gone unred bdout virtw vw2.

## Command

```bash
for r in ctl prov r698 r703 bdw pull gone unred bdout virtw vw2; do
  for f in LLM/evidence/02.766/before/compare.txt LLM/evidence/02.768/before/compare.txt; do
    awk -v r="== $r" '$0==r{p=1;print;next} /^== /{p=0} p' "$f" > /var/tmp/768raw/blk.$(basename $(dirname $(dirname $f))).$r
  done
  cmp -s /var/tmp/768raw/blk.02.766.$r /var/tmp/768raw/blk.02.768.$r && echo "same $r" || echo "DIFFERENT $r"
done
```

## Expected output

eleven lines `same <root>`.

## Actual output (verbatim)

```
DIFFERENT ctl
DIFFERENT prov
DIFFERENT r698
DIFFERENT r703
DIFFERENT bdw
DIFFERENT pull
DIFFERENT gone
DIFFERENT unred
DIFFERENT bdout
DIFFERENT virtw
DIFFERENT vw2
```

Block header form in both files is `== <root>` (the awk pattern matched; blocks are non-empty).

Diff of the `ctl` block, verbatim (`diff -u /var/tmp/768raw/blk.02.766.ctl /var/tmp/768raw/blk.02.768.ctl`):

```
@@ -1,11 +1,14 @@
 == ctl
   fast: rc=0 pg_lines=0
+    provider versions {}
     refusals []
     reinstalls []
   df: rc=0 pg_lines=0
+    provider versions {}
     refusals []
     reinstalls []
   pg: rc=0 pg_lines=2668
+    provider versions {}
     refusals []
     reinstalls []
   provider-version differences fast vs df: []
```

Observation (not used to decide the rule): the 768 block differs from the 766 block only by the
`provider versions` lines. With those lines removed (`grep -v 'provider versions'`), all eleven
blocks are byte-identical. The 768 `compare.py` is identical to `LLM/evidence/02.766/compare.py`
except for the roots path (`diff -u` shows one changed line, `roots = '/var/tmp/768roots'`).
`LLM/evidence/02.766/before/compare.txt` contains zero `provider versions` lines (its script
prints them), so the committed 766 capture predates the current `compare.py` output.
This was not corrected: it is reported, not worked around.

## What had been changed so far

- Branches `backlog/768-769-fast-plan-policy` created in both repos from `backlog/764-build-closure`.
- `LLM/evidence/02.768/` created: `before/`, `after/`, copies of `02.766` probe scripts with paths retargeted to `768fx`, `768roots`, `768bin`; `mkroots.sh` got the optional SLOT argument in `vdb()` and the new roots (`a5use` dropped: provider has no IUSE).
- `LLM/evidence/02.768/before/NOTES.txt` written (commit shas, `a5use` and `a6` not-built lines, binary sha256).
- Frozen fixtures `/var/tmp/768fx`, frozen binary `/var/tmp/768bin/portuale`, roots `/var/tmp/768roots`, raw probes `/var/tmp/768raw/`.
- Probe captures saved under `LLM/evidence/02.768/before/` (probes, probes-2, probes-usepkg, compare, compare-usepkg).
- No product, test, fixture or commit change. Working tree left as it is; nothing committed.

## Resolution (coordinator, 2026-11-22)

Not a behaviour difference. `LLM/evidence/02.766/compare.py` prints a diagnostic
`provider versions` line (its line 98) that the committed `LLM/evidence/02.766/before/compare.txt`
does not carry: the capture was taken before that line was added, and both were committed
together in `0e2fb5c9`. With `provider versions` lines removed from both sides, all 11
blocks (ctl prov r698 r703 bdw pull gone unred bdout virtw vw2) are byte-identical
(checked by the coordinator with the same awk block extraction). STOP-1 is cleared; the
runbook continues. Root `a5use` was not built (the provider ebuild has no IUSE), as the
runbook's own drop rule says; it is recorded in `before/NOTES.txt`.
