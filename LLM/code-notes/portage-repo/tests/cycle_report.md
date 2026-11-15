# crates/portage-repo/src/tests/cycle_report.rs
Circular-dependency report pins against Portage captures.

## module
- The per-edge facts of the circular-dependency report. Expected values are Portage 3.0.83-r1 captures (`LLM/evidence/2026-10-23-506/`).

## fn json_report_lists_both_rotations_and_the_flag_solution
- Portage 3.0.83-r1, `LLM/evidence/2026-10-23-507/captures/01-use.portage.out`.
- Portage's JSON report shape for a USE-cycle fixture. Backlog #753:
  `usecyclea` bootstraps now, so the structure is pinned on the
  still-cycling two-flag shape (`cddseva` needs both flags off).

## fn json_report_carries_test_parents_masked_alternatives_and_truncation

- Backlog #753: `cddtsta` bootstraps now; the FEATURES=test parent pin
  moves to the still-cycling two-flag `test?`+`doc?` shape.

## fn a_test_flag_edge_names_its_owner_for_the_features_test_pointer

- Backlog #753: `cddtsta` bootstraps now; the FEATURES=test pointer
  pin moves to the still-cycling two-flag `test?`+`doc?` shape.

## fn edge_use_names_the_enabled_flag_that_pulls_the_dependency_in

- Backlog #753: `usecyclea` bootstraps now; the pin moves to the
  still-cycling two-flag shape.
