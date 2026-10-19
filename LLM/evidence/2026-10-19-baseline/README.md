# Baseline for the 2026-10-19 plan

Copied from `/var/tmp/s8/` before a container restart wiped tmpfs (2026-10-19).
They are the last runs of batch 2026-10-18 (portuale `b76ee705`, pmtest `19a2d8e`):

- `suite9.txt`: pmtest suite, 4102 passed, 37 skipped, 2 xfailed.
- `l0res.txt`: l0-resolver (reference 120 probes / 101 clean / 31 UNEXPLAINED).
- `l0fx.txt`: l0-fixture-oracle (48 probes, 0 UNEXPLAINED with `FX_HOST_RUNNING_ROOT=1`).

The suite log has dots only, no test names: W0 of the plan must re-run it
with `-rA` or `--junitxml` to get a by-name baseline.

W0 (2026-10-19, portuale `ba0b184a`): pmtest suite twice, 4108 passed / 37 skipped / 2 xfailed, 0 failures both times
(`suite-by-name.xml` is the junit of the first run, use it for by-name comparison). `cargo test --release` three
times, all rc 0 (1161 unit tests, ~194 s). Closes #376 and #407.
