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
