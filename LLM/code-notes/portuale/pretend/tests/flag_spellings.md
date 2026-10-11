# crates/portuale/src/pretend/tests/flag_spellings.rs
`=True` spellings of eleven options behave exactly like the bare flags.

## fn verbose_true_spelling_behaves_like_the_bare_flag
- Backlog #241: Portage's `--verbose` is `true_y_or_n` choices (`main.py:732-735`, `in true_y` at `:1128-1131`).

## fn quiet_true_spelling_behaves_like_the_bare_flag
- Backlog #241: Portage's `--quiet` is `true_y_or_n` choices (`main.py:612-614`, `in true_y` at `:939-941`).

## fn deselect_true_spelling_behaves_like_the_bare_flag
- Backlog #241: Portage's `--deselect` is `true_y_or_n` choices (`main.py:446-449`, `in true_y` at `:869-870`); the bare form stays the standalone deselect action either way.

## fn changed_deps_true_spelling_behaves_like_the_bare_flag
- Backlog #241: Portage's `--changed-deps` is `true_y_or_n` choices (`main.py:405-407`, `in true_y` at `:848-851`).

## fn changed_deps_report_true_spelling_behaves_like_the_bare_flag
- Backlog #241: Portage's `--changed-deps-report` is `true_y_or_n` choices (`main.py:409-411`, `in true_y` at `:854-857`).

## fn selective_true_spelling_behaves_like_the_bare_flag
- Backlog #241: Portage's `--selective` is `true_y_or_n` choices (`main.py:697`, `in true_y` at `:988-989`).

## fn changed_slot_true_spelling_behaves_like_the_bare_flag
- Backlog #241: Portage's `--changed-slot` is `true_y_or_n` choices (`main.py:413-415`, `in true_y` at `:860-863`).

## fn quiet_build_true_spelling_behaves_like_the_bare_flag
- Backlog #241: Portage's `--quiet-build` is `true_y_or_n` choices (`main.py:617-619`, `in true_y` at `:944-945`); inert under `--pretend` either way.

## fn with_test_deps_true_spelling_behaves_like_the_bare_flag
- Backlog #241: Portage's `--with-test-deps` is `true_y_or_n` choices (`main.py:745-747`, `in true_y` at `:1133-1136`).

## fn autounmask_keep_keywords_true_spelling_behaves_like_the_bare_flag
- Backlog #241: Portage's `--autounmask-keep-keywords` is `true_y_or_n` choices (`main.py:365-367`, `in true_y` at `:821-822`) -- the old wrong `(choose from "y", "n")` rejection of `=True` is gone, and the bare form (once a "requires an argument" rc 2) now keeps keywords like Portage's inserted `"True"`. `=True` keeps keywords, so the run falls back to the older stable `akk0a-1` like Portage (rc 0, same as the `=y` pin since backlog #198's missing-dep backtrack).

## fn autounmask_keep_masks_true_spelling_behaves_like_the_bare_flag
- Backlog #241: Portage's `--autounmask-keep-masks` is `true_y_or_n` choices (`main.py:369-371`, `in true_y` at `:824-825`) -- same rework as keep-keywords. `=True` keeps the mask, so the package.mask'd target stays fatal (rc 1, the default).

## fn true_spelling_matches_bare (in mod.rs, shared helper)
- Backlog #241: Portage's `true_y_or_n` choices (`_emerge/main.py:320-322,365-371,405-415,446-449,612-623,697,732-747`; normalized `in true_y` per option) admit `=True` for eleven options portuale parsed narrowly (`=y`/`=n` only, rc 2 `unrecognized option` for nine of them and a wrong `(choose from "y", "n")` rejection for the two autounmask-keep options); `=True` must behave exactly like the bare flag. `expected_rc` pins the shared exit code (0 for the resolving shapes, 1 for the two autounmask-keep shapes whose fixtures fail like Portage). `--pretend` never writes, so sharing one `ROOT` across both runs is hermetic.
