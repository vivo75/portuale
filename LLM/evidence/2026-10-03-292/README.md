# Evidence 2026-10-03 — #292 (withhold vs conflict block order) and #291 (second half)

Container probe, real 3.0.82.2 vs portuale (`oc-292/probe/probe-host.sh`, main pmtest 672fe60, portuale b726fbb3 + the #292 patch).

## #292 — `cells292.txt`, `before.txt`, `after.txt`
Installed `whtarget-1.0` (world; `RDEPEND=~whblocker-1.0`), `whblocker-1.0`, `whpuller-1.0`; argv `--pretend --update --deep --newuse --oneshot [--backtrack 0] <whpuller + the old/new consumer pair, either order>`.
Real: the conflict block (`slotconflicttarget`) first, the pin-withhold block (`whblocker`) second, in all four cells (`backtrack: 0/20`, no restart). Portuale before: withhold first at the default budget (its backtrack mask also carries `whblocker`, and mask rows chain first); bt0 already matched. After: all four cells match. Pin: `test_oracle_292_pin_withhold_block_follows_the_conflict_block`.

## #291 second half — `291-bare-arg-cells.txt`, `291-bare-pkg-cells.txt`
Hypothesis: a non-discriminating parent (an atom matching both instances) is filed under every instance, so a surviving lone conflict at bt0 gains a duplicate parent line, where real has one `all_parent_atoms` list. 12 cells (`slotconflictunsolvable` + a bare `slotconflicttarget` argument, or + `slotconflictnewconsumer` whose RDEPEND is the bare atom; bt0 and default; both argument orders): no duplicate line on either side; real lists no bare-atom parent under any instance. Nothing to fix; no pin added.
Side findings, both pre-existing, not touched here (filed as #298): `N1b`/`N1bz` instance order (real 1.0 then 2.0 when the bare-atom consumer is walked first, portuale 2.0 then 1.0); `N2` which of two same-instance parents shows first (real oldpin, portuale oldconsumer; real's within-instance order is set order).
