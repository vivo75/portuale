# crates/portage-repo/src/tests/tests_195d.rs
Operator-level pins for `violated_parent_flags` against Portage's `Atom.violated_conditionals`.

## module
- Backlog #195, round 4 (g195d Important-1): pins against Portage's `Atom.violated_conditionals` (`lib/portage/dep/__init__.py:1465`, branches `:1525-1576`).
- Oracle: `LLM/evidence/2026-09-28-g195/g195d-oracle-output.txt` (a host run of Portage 3.0.82.2 on these same inputs via `g195d-oracle-probe.py`). Each row's expectation is the oracle's `arm` column -- the parent arm's composed end-to-end outcome, not just the in-isolation `conditional` hit: `SKIP-missing-iuse` rows (invalid defaultless conditionals, which Portage never calls the function for -- `depgraph.py:6721-6727`) and `FAIL-gate` rows both expect `[]`, exactly what the port returns through its poison and concrete-gate paths.

## fn cell_195d
- `token` is the full `[...]` entry (e.g. `"foo(+)?"`), `valid` whether `foo` is in child IUSE, `p` / `c` the parent/child USE bits.
- Portage's `Package.py:728-731` filters child USE to valid IUSE, so an invalid flag is never set -- the same contract `violated_parent_flags` documents.

## fn violated_parent_flags_matches_portage_across_the_oracle_matrix
- Invalid rows only carry C=0, since filtered child USE never holds an invalid flag.
- `flag?` -- Portage `:1525-1531`.
- Invalid `flag?`: defaultless poisons via the `missing_iuse`-equivalent path.
- `flag=` -- Portage `:1532-1549`.
- Invalid `flag=`: the `(+)` parent-lacks row violates via the `missing_enabled` sub-branch.
- `flag!=` -- Portage `:1550-1567`.
- Invalid `!foo(+)=` parent-has-child-lacks: the review's corner, Portage's `missing_enabled` sub-branch.
- `!flag?` -- Portage `:1568-1576`.

## fn gate_195d
- `bar_valid` / `bar_use` mirror the oracle's columns.
- Controls are oracle-verified `[foo]`, gate PASS.
- History: the port's valid-only gate used to let the invalid `(-)`/`(+)` rows through.
- Invalid defaultless unconditionals: Portage skips the arm via `missing_iuse`.
