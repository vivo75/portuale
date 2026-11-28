# crates/portuale/src/pretend/tests/bdeps_presence.rs
Context moved out of the code comments.

## module
- Backlog numbers removed from the comment (original text): #361 S1: `bdeps=auto` is a `--usepkg` key-presence rule, not a value rule.  Portage sets `myparams["bdeps"] = "auto"` whenever `--with-bdeps` is absent, `--with-bdeps-auto` is not `n`, and the `--usepkg` key is absent (`create_depgraph_params.py:97-103`); `_emerge/main.py:1109-1114` stores `--usepkg=n` as `False`, so the key is present and an explicit `=n` turns bdeps-auto off exactly like bare `--usepkg`. Implications that insert the key live in `_emerge/actions.py:3709-3723` (`FEATURES` fold, `--getbinpkgonly`/`--getbinpkg`/`--usepkgonly` chain). Expected values below are Portage 3.0.82.2's rows from `LLM/evidence/2026-10-15-359-s0/` (`matrix.md` group C, plus the reused `361-blib-pvuD[-noflags].txt` cells in `../2026-10-15-plan-359/`).

## fn usepkgonly_n_leaves_bdeps_auto_on
- Backlog numbers removed from the comment (original text): S0 `s0-c-usepkgonly-n.txt`: the bdeps rule (`create_depgraph_params.py:97-103`) tests only the `--usepkg` key, and `--usepkgonly=n` (stored `False`) implies nothing (`actions.py` implications need `is True`), so bdeps-auto stays on. Portage still prints `Total: 0` there, but through the ebuild-db drop (`depgraph.py:809`, `"--usepkgonly" not in myopts`), which `EmergeOpts::ebuild_db_dropped` models (#396) — not a bdeps rule.

## fn usepkgonly_n_keeps_the_key_so_the_ebuild_database_is_dropped
- Backlog numbers removed from the comment (original text): #396, Portage `depgraph.py:809`: the key stays present as `False`.

## fn update_if_installed_implies_update
- Backlog numbers removed from the comment (original text): #375, Portage `create_depgraph_params.py:139-143`.
