# crates/portage-repo/src/tests/tests_consumer_or_view.rs
An installed consumer's `||` is chosen the way `dep_zapdeps` chooses it, in the view Portage uses (#580 10b).

## module
- Oracle: Portage 3.0.83 on scratch copies of the fixtures, captures in `LLM/evidence/2026-10-26-575-residues/t10b/captures/` (`t1-*`, `t2-*`, `t3-*`, `q19-order-*`, each `*-portage.txt` with its `*-portuale-before.txt` and `*-portuale.txt`). Probe scripts `run-pair.sh`, `probe-*.sh` in the same folder.
- `t2_swap_slotop_matches_portage` is `#[ignore]`: Portage's T2 conflict comes from the built `prov:0/5.42=` atom alone (D:8803-8834), which portuale does not model.
- `sibling_virtuals_share_the_atom_a_sibling_chose` passes on HEAD too (the installed-arm rule gave the same rows); it guards the Q19 pre-pass.
