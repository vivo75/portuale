use super::*;

#[test]
fn apply_incremental_is_reusable_for_per_package_use_overrides() {
    let mut set = HashSet::from(["foo".to_string()]);
    apply_incremental("flag1 -foo +flag2", &mut set);
    assert_eq!(
        set,
        HashSet::from(["flag1".to_string(), "flag2".to_string()])
    );
}
