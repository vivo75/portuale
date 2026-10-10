use super::*;

#[test]
fn verbose_true_spelling_behaves_like_the_bare_flag() {
    // Backlog #241: real `--verbose` is `true_y_or_n` choices
    // (`main.py:732-735`, `in true_y` at `:1128-1131`).
    true_spelling_matches_bare("--verbose=True", "--verbose", "dev-libs/newpkg", 0);
}

#[test]
fn quiet_true_spelling_behaves_like_the_bare_flag() {
    // Backlog #241: real `--quiet` is `true_y_or_n` choices
    // (`main.py:612-614`, `in true_y` at `:939-941`).
    true_spelling_matches_bare("--quiet=True", "--quiet", "dev-libs/newpkg", 0);
}

#[test]
fn deselect_true_spelling_behaves_like_the_bare_flag() {
    // Backlog #241: real `--deselect` is `true_y_or_n` choices
    // (`main.py:446-449`, `in true_y` at `:869-870`); the bare form
    // stays the standalone deselect action either way.
    true_spelling_matches_bare("--deselect=True", "--deselect", "dev-libs/foo", 0);
}

#[test]
fn changed_deps_true_spelling_behaves_like_the_bare_flag() {
    // Backlog #241: real `--changed-deps` is `true_y_or_n` choices
    // (`main.py:405-407`, `in true_y` at `:848-851`).
    true_spelling_matches_bare(
        "--changed-deps=True",
        "--changed-deps",
        "dev-libs/changeddepspkg",
        0,
    );
}

#[test]
fn changed_deps_report_true_spelling_behaves_like_the_bare_flag() {
    // Backlog #241: real `--changed-deps-report` is `true_y_or_n`
    // choices (`main.py:409-411`, `in true_y` at `:854-857`).
    true_spelling_matches_bare(
        "--changed-deps-report=True",
        "--changed-deps-report",
        "dev-libs/changeddepspkg",
        0,
    );
}

#[test]
fn selective_true_spelling_behaves_like_the_bare_flag() {
    // Backlog #241: real `--selective` is `true_y_or_n` choices
    // (`main.py:697`, `in true_y` at `:988-989`).
    true_spelling_matches_bare("--selective=True", "--selective", "dev-libs/samepkg", 0);
}

#[test]
fn changed_slot_true_spelling_behaves_like_the_bare_flag() {
    // Backlog #241: real `--changed-slot` is `true_y_or_n` choices
    // (`main.py:413-415`, `in true_y` at `:860-863`).
    true_spelling_matches_bare(
        "--changed-slot=True",
        "--changed-slot",
        "dev-libs/changedslotpkg",
        0,
    );
}

#[test]
fn quiet_build_true_spelling_behaves_like_the_bare_flag() {
    // Backlog #241: real `--quiet-build` is `true_y_or_n` choices
    // (`main.py:617-619`, `in true_y` at `:944-945`); inert under
    // `--pretend` either way.
    true_spelling_matches_bare("--quiet-build=True", "--quiet-build", "dev-libs/newpkg", 0);
}

#[test]
fn with_test_deps_true_spelling_behaves_like_the_bare_flag() {
    // Backlog #241: real `--with-test-deps` is `true_y_or_n`
    // choices (`main.py:745-747`, `in true_y` at `:1133-1136`).
    true_spelling_matches_bare(
        "--with-test-deps=True",
        "--with-test-deps",
        "dev-libs/withtestdeppkg",
        0,
    );
}

#[test]
fn autounmask_keep_keywords_true_spelling_behaves_like_the_bare_flag() {
    // Backlog #241: real `--autounmask-keep-keywords` is
    // `true_y_or_n` choices (`main.py:365-367`, `in true_y` at
    // `:821-822`) -- the old wrong `(choose from "y", "n")`
    // rejection of `=True` is gone, and the bare form (once a
    // "requires an argument" rc 2) now keeps keywords like real's
    // inserted `"True"`. `=True` keeps keywords, so the run falls
    // back to the older stable `akk0a-1` like real (rc 0, same as
    // the `=y` pin since backlog #198's missing-dep backtrack).
    true_spelling_matches_bare(
        "--autounmask-keep-keywords=True",
        "--autounmask-keep-keywords",
        "dev-libs/akk0a",
        0,
    );
}

#[test]
fn autounmask_keep_masks_true_spelling_behaves_like_the_bare_flag() {
    // Backlog #241: real `--autounmask-keep-masks` is `true_y_or_n`
    // choices (`main.py:369-371`, `in true_y` at `:824-825`) --
    // same rework as keep-keywords. `=True` keeps the mask, so the
    // package.mask'd target stays fatal (rc 1, the default).
    true_spelling_matches_bare(
        "--autounmask-keep-masks=True",
        "--autounmask-keep-masks",
        "dev-libs/hardmaskedpkg",
        1,
    );
}
