use super::*;

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
