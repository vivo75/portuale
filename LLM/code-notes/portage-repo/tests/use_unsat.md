# crates/portage-repo/src/tests/use_unsat.rs
USE-unsatisfied candidate-classification tests.

## fn use_unsat_candidates_classify_change_use_and_missing_iuse
- Backlog #20: Portage's `_show_unsatisfied_dep`'s two reason kinds and its "Only show the latest version" rule.

## fn use_unsat_candidates_count_conditional_flags_toward_missing_iuse
- Backlog #265(a): Portage's `use.required` is every no-default flag, conditionals included (`lib/portage/dep/__init__.py:1363` `self.required = frozenset(no_default)`), so a conditional flag a visible candidate's own IUSE lacks is `Missing IUSE:`, never a `Change USE:` row and never a plain miss.
- Evidence: live Portage 3.0.82.2 staged-fixture probes (`LLM/evidence/2026-09-30-265/real/mia0a-{1,2}.txt`): `=dev-libs/mia0a-1` (`mia0b[foo?]`) prints the block with `- dev-libs/mia0b-1::testrepo (Missing IUSE: foo)`, and `=dev-libs/mia0a-2` (`mia0b[foo?,bar]`) prints the same row (not portuale's `Change USE: +bar`).
