use super::*;

#[test]
fn bind_slot_operator_binds_a_matched_equals_dep_and_leaves_the_rest_alone() {
    let root = TempDir::new("portuale-slotbind").keep();
    let vdb = root.join("var/db/pkg/dev-libs/foo-1.2");
    std::fs::create_dir_all(&vdb).unwrap();
    std::fs::write(vdb.join("CATEGORY"), "dev-libs\n").unwrap();
    std::fs::write(vdb.join("SLOT"), "4/9\n").unwrap();

    // A bare `:=` against an installed dep -> bound to its slot/sub-slot.
    assert_eq!(
        bind_slot_operator("dev-libs/foo:=", &root),
        "dev-libs/foo:4/9="
    );
    // An operator + version + `:=` -> only the slot dep is rewritten.
    assert_eq!(
        bind_slot_operator(">=dev-libs/foo-1:=", &root),
        ">=dev-libs/foo-1:4/9="
    );
    // An already-slotted `:2=` still rebinds (real `vardb.match` +
    // `with_slot`), but a slot the vdb doesn't have -> no match, bare.
    assert_eq!(
        bind_slot_operator("dev-libs/foo:4=", &root),
        "dev-libs/foo:4/9="
    );
    assert_eq!(
        bind_slot_operator("dev-libs/foo:7=", &root),
        "dev-libs/foo:7="
    );
    // Not a `:=` operator, and a `:=` dep with nothing installed:
    // both untouched.
    assert_eq!(
        bind_slot_operator("dev-libs/foo:4", &root),
        "dev-libs/foo:4"
    );
    assert_eq!(
        bind_slot_operator("dev-libs/bar:=", &root),
        "dev-libs/bar:="
    );
    // Not an atom at all (a `||`-group token).
    assert_eq!(bind_slot_operator("||", &root), "||");

    let _ = std::fs::remove_dir_all(&root);
}
