use super::*;

#[test]
fn required_set_reachable_cps_follows_the_forward_installed_dep_graph() {
    // Throwaway vdb: tail -> mid -> target chain, an island, and a @system half.
    let dir = TempDir::new("portage-repo-reach").keep();
    let mk = |name: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
        fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
    };
    mk("tail-1.0", "dev-libs/mid:0/1=");
    mk("mid-1.0", "dev-libs/target:0/1=");
    mk("target-1.0", "");
    mk("island-1.0", "");
    mk("sysmember-1.0", "dev-libs/sysdep");
    mk("sysdep-1.0", "");

    let cp = |p: &str| ("dev-libs".to_string(), p.to_string());

    // Only `tail` in @world: the chain is reachable, the rest is not.
    let r = required_set_reachable_cps(&dir, &["dev-libs/tail".to_string()], &[]);
    assert!(r.contains(&cp("tail")) && r.contains(&cp("mid")) && r.contains(&cp("target")));
    assert!(!r.contains(&cp("island")));
    assert!(!r.contains(&cp("sysmember")) && !r.contains(&cp("sysdep")));

    // `sysmember` in @system pulls `sysdep`.
    let r = required_set_reachable_cps(&dir, &[], &["dev-libs/sysmember".to_string()]);
    assert!(r.contains(&cp("sysmember")) && r.contains(&cp("sysdep")));
    assert!(!r.contains(&cp("mid")));

    let _ = fs::remove_dir_all(&dir);
}
