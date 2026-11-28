# crates/portage-repo/src/tests/tests_p563.rs
The perl-5.44 stabilization shape (#563 heal) and the skip-block pin forms (#568/#569), hermetically.

## module
- Oracle: Portage 3.0.83 host runs saved under `LLM/evidence/2026-10-25-563/captures/`; L0 log `../pmtest/differential-test-bed/logs/l0-20261025T084209Z` (`dev-perl_XML-Parser` probe; `dev-lang_perl` is the bare-provider twin).
- Skip-block oracle `LLM/evidence/2026-10-26-568/` (`probe-p569.sh` + `NOTES.md`).

## fn p563_pull_heals_with_skip_block_and_backtrack_notice
- Portage oracle (`captures/portage-pull-bare.*`). Hermetic oracle `LLM/evidence/2026-10-26-568/NOTES.md` (#568).

## fn skip_block_pins_keep_the_built_subslot_atom
- Portage `_show_missed_update` over the bintree+vdb. Hermetic capture `LLM/evidence/2026-10-26-568/captures/portage-p569pull.stderr`; bed `l0-20261025T111515Z` Portage side, XML-Parser probe: 48 real-only `:0/5.42=` lines.

## fn g0_benign_disclosure_blocks_the_dropped_pin_heal
- S0 capture `LLM/evidence/2026-10-25-570/` (`portage-g0combo`: rc 1, backtrack 1/20, only the masked disclosure; the `p569pull` residual alone heals rc 0, backtrack 2/20).

## fn skip_block_pins_skip_non_selected_or_branches
- Hermetic oracle: `LLM/evidence/2026-10-26-568/NOTES.md`.

## fn plain_pull_mirrors_the_scan_filing_into_the_replace_display_set
- Backlog numbers removed from the comment (original text): Backlog #608 (`-pvD dev-libs/p569pull`, oracle `D-p569pull-portage.txt`): one backtrack, and the slot-operator replace set names both the installed consumer and the provider whose installed child's slot atom matches the upgrade row (`r U`, not plain `U`). The rebuild scan (`fold_rebuild_scan`) files the logic set while the conflict-ABI probe vetoes without `--update`, so the display mirror (`probe_replaced`) must track the scan's filing too. Portage: depgraph.py, _slot_operator_trigger_reinstalls / _gen_reinstall_sets (`__auto_slot_operator_replace_installed__`). `-D` matches the oracle invocation; without it the shallow walk costs an extra restart while the display set is the same.
