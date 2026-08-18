use super::*;

#[test]
fn overlay_package_use_mask_and_force_are_scoped_with_no_masters_merge() {
    check_overlay_package_use_scoping("", |c| (&c.package_use_mask, &c.package_use_force));
}

#[test]
fn overlay_package_use_stable_mask_and_force_get_the_same_scoping() {
    check_overlay_package_use_scoping(".stable", |c| {
        (&c.package_use_stable_mask, &c.package_use_stable_force)
    });
}
