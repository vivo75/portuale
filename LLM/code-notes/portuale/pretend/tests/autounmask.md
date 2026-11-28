# crates/portuale/src/pretend/tests/autounmask.rs
Context moved out of the code comments.

## fn autounmask_proposes_unmask_for_masked_dep
- Backlog numbers removed from the comment (original text): Backlog #445: `--pretend --autounmask dev-libs/maskneedpkg` proposes the unmask for the package.mask'd dependency instead of the plain masked block. Expected value from Portage 3.0.82.2 (host staged-fixture probe, `PORTAGE_CONFIGROOT`/`ROOT` at the staged fixtures, `PYTHONHASHSEED=0`): rc 1, one `[ebuild N #]` row per package, the `package.unmask` changes block with the `# required by` chain, and the `--autounmask-keep-masks` NOTE. Backlog #462: the header (`These are the packages…` / `Calculating…` / `Dependency resolution took…`, wall-clock seconds cut per the determinism convention) prints before the rows, and the unmask block carries Portage's `getmaskingreason` lines (`# <file>:` + the file's comment block). Oracle: `LLM/evidence/2026-10-21-f462/portage.txt` cell c1.

## fn autounmask_n_and_keep_masks_y_keep_the_plain_masked_block
- Backlog numbers removed from the comment (original text): Review focus for #445: `--autounmask=n` and `--autounmask-keep-masks=y` on a masked-only dep keep the plain masked block (no rows, no unmask proposal). Same Portage 3.0.82.2 probe grounding as above.
