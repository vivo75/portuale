# crates/portage-repo/src/tests/use_unsat.rs
USE-unsatisfied candidate-classification tests.

## fn use_unsat_candidates_classify_change_use_and_missing_iuse
- Backlog #20: Portage's `_show_unsatisfied_dep`'s two reason kinds and its "Only show the latest version" rule.

## fn use_unsat_candidates_count_conditional_flags_toward_missing_iuse
- Backlog #265(a): Portage's `use.required` is every no-default flag, conditionals included (`lib/portage/dep/__init__.py:1363` `self.required = frozenset(no_default)`), so a conditional flag a visible candidate's own IUSE lacks is `Missing IUSE:`, never a `Change USE:` row and never a plain miss.
- Evidence: live Portage 3.0.82.2 staged-fixture probes (`LLM/evidence/2026-09-30-265/real/mia0a-{1,2}.txt`): `=dev-libs/mia0a-1` (`mia0b[foo?]`) prints the block with `- dev-libs/mia0b-1::testrepo (Missing IUSE: foo)`, and `=dev-libs/mia0a-2` (`mia0b[foo?,bar]`) prints the same row (not portuale's `Change USE: +bar`).

## fn a_masked_version_with_the_required_iuse_hands_the_miss_to_the_masked_block
- Backlog numbers removed from the comment (original text): Backlog #611(a): visible `r594xdep-1` lacks `r594flag`, keyword-masked `r594xdep-2` declares it. Portage 3.0.83 (`emerge -p dev-libs/r594xtop`, `LLM/evidence/2026-11-06-sweep/611a/`) prints the all-masked block with the `-2` row, not `Missing IUSE` (`masked_with_iuse`, D:6918-6929).

## fn show_masked_packages_prints_the_mask_file_comment_once_and_license_paths
- Backlog numbers removed from the comment (original text): Backlog #611(c): Portage's `show_masked_packages` (depgraph.py D:12632-12690) prints, under a `package.mask` row, the masking file and the comment above the entry (once per distinct comment), and a line per missing license found under a repo's `licenses/`. Portage 3.0.83 prints the fixture's user `package.mask` comment under `maskeddep-1.0` (`LLM/evidence/2026-11-06-sweep/611c/`).
