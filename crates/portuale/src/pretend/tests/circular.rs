use super::*;

#[test]
fn circular_node_text_matches_portage_package_str() {
    // Backlog #206 S1: real `_emerge/Package.py::Package.__str__`
    // for a merge-bound ebuild — `(cpv:slot/sub::repo, ebuild
    // scheduled for merge)` — byte for byte, grounded on the live
    // n206 probe (`=dev-libs/cyc0b-1`, `dev-libs/hardcyclea`,
    // `dev-libs/usecyclea`, all `ROOT=$FX` with no `to` suffix).
    let nc = Colorizer::new(false);
    assert_eq!(
        circular_node_text("dev-libs/cyc0b-1", "0", "0", "testrepo", "ebuild", &nc),
        "(dev-libs/cyc0b-1:0/0::testrepo, ebuild scheduled for merge)"
    );
    // A non-0 slot renders `:slot/sub_slot`, like real
    // `_append_slot`-less `__str__` (which always prints both).
    assert_eq!(
        circular_node_text("dev-libs/slotted-2.0", "1", "2", "testrepo", "ebuild", &nc),
        "(dev-libs/slotted-2.0:1/2::testrepo, ebuild scheduled for merge)"
    );
    // A binary merge says `binary`, like real `type_name`.
    assert_eq!(
        circular_node_text("dev-libs/binpkg-1.0", "0", "0", "testrepo", "binary", &nc),
        "(dev-libs/binpkg-1.0:0/0::testrepo, binary scheduled for merge)"
    );
    // Under `--color y` real wraps the `cpv:slot/sub::repo` head in
    // `PKG_MERGE` (`PKG_BINARY_MERGE` for a binary merge); the
    // `, <type> scheduled for merge)` tail stays plain.
    let yc = Colorizer::new(true);
    assert_eq!(
        circular_node_text("dev-libs/cyc0b-1", "0", "0", "testrepo", "ebuild", &yc),
        "(\x1b[32mdev-libs/cyc0b-1:0/0::testrepo\x1b[39;49;00m, ebuild scheduled for merge)"
    );
    assert_eq!(
        circular_node_text("dev-libs/binpkg-1.0", "0", "0", "testrepo", "binary", &yc),
        "(\x1b[35mdev-libs/binpkg-1.0:0/0::testrepo\x1b[39;49;00m, binary scheduled for merge)"
    );
}
