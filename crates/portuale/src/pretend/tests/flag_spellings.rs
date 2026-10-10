use super::*;

#[test]
fn verbose_true_spelling_behaves_like_the_bare_flag() {
    // Portage `true_y_or_n`: `--verbose=True` behaves like bare `--verbose`.
    true_spelling_matches_bare("--verbose=True", "--verbose", "dev-libs/newpkg", 0);
}

#[test]
fn quiet_true_spelling_behaves_like_the_bare_flag() {
    // Portage `true_y_or_n`: `--quiet=True` behaves like bare `--quiet`.
    true_spelling_matches_bare("--quiet=True", "--quiet", "dev-libs/newpkg", 0);
}

#[test]
fn deselect_true_spelling_behaves_like_the_bare_flag() {
    // Portage `true_y_or_n`: `--deselect=True` behaves like bare `--deselect`.
    true_spelling_matches_bare("--deselect=True", "--deselect", "dev-libs/foo", 0);
}

#[test]
fn changed_deps_true_spelling_behaves_like_the_bare_flag() {
    // Portage `true_y_or_n`: `--changed-deps=True` behaves like the bare flag.
    true_spelling_matches_bare(
        "--changed-deps=True",
        "--changed-deps",
        "dev-libs/changeddepspkg",
        0,
    );
}

#[test]
fn changed_deps_report_true_spelling_behaves_like_the_bare_flag() {
    // Portage `true_y_or_n`: `--changed-deps-report=True` behaves like the bare flag.
    true_spelling_matches_bare(
        "--changed-deps-report=True",
        "--changed-deps-report",
        "dev-libs/changeddepspkg",
        0,
    );
}

#[test]
fn selective_true_spelling_behaves_like_the_bare_flag() {
    // Portage `true_y_or_n`: `--selective=True` behaves like bare `--selective`.
    true_spelling_matches_bare("--selective=True", "--selective", "dev-libs/samepkg", 0);
}

#[test]
fn changed_slot_true_spelling_behaves_like_the_bare_flag() {
    // Portage `true_y_or_n`: `--changed-slot=True` behaves like the bare flag.
    true_spelling_matches_bare(
        "--changed-slot=True",
        "--changed-slot",
        "dev-libs/changedslotpkg",
        0,
    );
}

#[test]
fn quiet_build_true_spelling_behaves_like_the_bare_flag() {
    // Portage `true_y_or_n`: `--quiet-build=True` behaves like the bare flag.
    true_spelling_matches_bare("--quiet-build=True", "--quiet-build", "dev-libs/newpkg", 0);
}

#[test]
fn with_test_deps_true_spelling_behaves_like_the_bare_flag() {
    // Portage `true_y_or_n`: `--with-test-deps=True` behaves like the bare flag.
    true_spelling_matches_bare(
        "--with-test-deps=True",
        "--with-test-deps",
        "dev-libs/withtestdeppkg",
        0,
    );
}

#[test]
fn autounmask_keep_keywords_true_spelling_behaves_like_the_bare_flag() {
    // Portage `true_y_or_n`: `=True` keeps keywords, resolving to the older stable.
    true_spelling_matches_bare(
        "--autounmask-keep-keywords=True",
        "--autounmask-keep-keywords",
        "dev-libs/akk0a",
        0,
    );
}

#[test]
fn autounmask_keep_masks_true_spelling_behaves_like_the_bare_flag() {
    // Portage `true_y_or_n`: `=True` keeps the mask, so the masked target stays fatal.
    true_spelling_matches_bare(
        "--autounmask-keep-masks=True",
        "--autounmask-keep-masks",
        "dev-libs/hardmaskedpkg",
        1,
    );
}
