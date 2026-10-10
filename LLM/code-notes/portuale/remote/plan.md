# crates/portuale/src/remote/plan.rs
The remote entry points: resolve, plan, and run each unit.

## module
- Resolve-path support (slice 5).

## fn run_remote_resolve
- Fail-early stage 2 (plan §5.5): client sanity before resolving.
- S8.1 binary gates (read-only): the plan rides the handoff into `run_remote_plan`, which installs only when units ship. Local transport without the override skips the gates and keeps `current_exe()` (#326 D5).

## fn drop_dependents
- The `run_merge_loop` `_calc_resume_list` policy.

## fn run_remote_plan
- Sequential; `--remote-jobs` stays a reserved knob. Report is per-unit trailers (the flow's `>>> Remote merged <cpv>` line, plus `!!! Remote <cpv>: failed: …` and `>>> Remote <cpv>: skipped (…)` from the loop) with a closing `>>> Remote summary: …` line (plan §11). With `--keep-going` the loop returns a combined failed+skipped report (the `run_merge_loop` `_calc_resume_list` policy).
- Backlog #322: every shipped unit carries the vendored phase runtime (`build_bundle`); the gate stage runs before the vdb shadow, the ledger gate, any ssh and any write, instead of panicking after the plan is printed.
- Vdb shadow for the pre-ship pre-check (plan §7). Server ledger base (plan §8). Strict-mode provenance gate (plan §8).
- Portage `preinst_mask()` (`bin/misc-functions.sh`): backlog #170. `ConfigPlacement::Client` resolves the pulled client `/etc/portage`; `ConfigPlacement::Server` the server's own (both are `config` here: the resolve already ran under `ConfigRootOverride` for the placed root).
- Backlog #171: the local `refresh_features` rule. Backlog #171 review: the scrubbed vdb env records the client's configured compressor (e.g. `lbzip2`), not always the `make.globals` default.
- S8.2 install: after the read-only stages (the shadow, the ledger gate and the resolve above); never with an empty plan. `--pretend` never reaches here (`mrg` routes it locally with the remote options dropped).

## fn run_remote_plan (loop)
- #72 B3: a removal is not remotely merged (execution is a non-goal).

## fn run_remote
- Slice-1 entry point; slice 2+ entry point with `--remote-binpkg`. Backlog #322: an explicit `--remote-binpkg` bundles the phase runtime into the unit.
- The trial path enforces the S8.1 binary gates like the resolve path.
