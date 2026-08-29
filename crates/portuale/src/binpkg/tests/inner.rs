use super::*;

#[test]
fn classify_inner_member_matches_portage_extract_filename_compression() {
    assert!(matches!(
        classify_inner_member("metadata", "metadata.tar"),
        Some(None)
    ));
    assert!(matches!(
        classify_inner_member("metadata", "metadata.tar.zst"),
        Some(Some(_))
    ));
    assert!(matches!(
        classify_inner_member("metadata", "metadata.tar.xz"),
        Some(Some(_))
    ));
    assert!(classify_inner_member("metadata", "metadata.tar.zst.sig").is_none());
    assert!(classify_inner_member("metadata", "image.tar.zst").is_none());
    assert!(classify_inner_member("metadata", "Manifest").is_none());
}
