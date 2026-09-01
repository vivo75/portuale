use super::*;

#[test]
fn read_gpkg_metadata_rejects_a_non_gpkg_tar() {
    // A flat top-level tar fails the one-level structure check before
    // any marker lookup.
    let scratch = ScratchDir::new("gpkg-negtest").unwrap();
    let junk = scratch.path().join("a.txt");
    fs::write(&junk, b"x").unwrap();
    let flat = scratch.path().join("plain.tar");
    run_tar(&["-cf", &lossy(&flat), "-C", &lossy(scratch.path()), "a.txt"]).unwrap();
    let err = read_gpkg_metadata(&flat).unwrap_err();
    assert!(err.is_invalid(), "{err:?}");
    assert!(
        err.message().contains("gpkg file structure mismatch"),
        "{err}"
    );

    // A structurally valid container with no `gpkg-1` marker fails the
    // inner-tar lookup guard.
    let g = build_gpkg("foo-1.0", &[("metadata.tar", b"x")], None);
    let err = read_gpkg_metadata(&g).unwrap_err();
    assert!(err.is_invalid(), "{err:?}");
    assert!(err.message().contains("gpkg-1"), "{err}");
}
