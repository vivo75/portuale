use super::*;

#[test]
fn binrepo_packages_dir_maps_scheme_to_the_portage_edb_cache_layout() {
    let root = Path::new("/eroot");
    let http = BinRepo {
        name: "h".into(),
        sync_uri: "https://gpkg.example.org/seed-desk".into(),
        priority: 0,
        location: None,
        verify_signature: true,
        frozen: false,
    };
    assert_eq!(
        http.packages_dir(root),
        Path::new("/eroot/var/cache/edb/binhost/gpkg.example.org/seed-desk")
    );
    let file = BinRepo {
        name: "f".into(),
        sync_uri: "file:///srv/pkgs".into(),
        priority: 0,
        location: None,
        verify_signature: true,
        frozen: false,
    };
    assert_eq!(file.packages_dir(root), Path::new("/srv/pkgs"));
}

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
