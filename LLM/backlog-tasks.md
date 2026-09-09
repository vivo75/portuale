## Tier 4 — `--solver=` real-tree correctness (pubgrub / resolvo)

33. **`--solver=resolvo` cycle linearization** — `install_order` can't order any closure with a toolchain cycle (glibc↔gcc↔perl); `solver_bridge.rs` prints raw ids. Non-functional on real targets. `docs/history/solver-backends-analysis.md`. [J]
34. **`--solver=pubgrub` over-merge** — feeds pubgrub the over-approximated `flag?()` reachability closure as the real graph (`nodejs`: 48 vs 8). Feed the resolved graph instead. `solver_bridge.rs`. [J]

