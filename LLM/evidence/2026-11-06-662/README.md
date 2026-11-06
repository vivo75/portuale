# #662 — `!!! The following installed packages are masked:` (2026-11-06)

Portage 3.0.83 via `differential-test-bed/run/l0-fixture-oracle.sh` on
`--pretend dev-libs/omasked`, `--pretend --verbose dev-libs/omasked`,
`--pretend --update dev-libs/omasked`, plus `--quiet` and `--tree` (`real/`), next to portuale after the fix
(`portuale/`). All three rc 0. The owed check on the plain row is answered:
Portage prints the block there too, so the old contract pin
`stderr == ""` was wrong and is replaced. Portage's `Performing Global Updates`
noise and `Undefined license group` lines come from the bed's first run, not
from the product.
