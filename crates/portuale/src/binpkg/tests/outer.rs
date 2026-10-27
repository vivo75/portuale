use super::*;

/// A symlinked `Manifest`: all three sites reject it by type, before
/// any read.
#[test]
fn regular_outer_member_check_rejects_a_symlinked_manifest() {
    let (_guard, g) = build_gpkg_entries(&[
        ("pkg-1.0/gpkg-1", GpkgEntry::File(b"")),
        ("pkg-1.0/metadata.tar.zst", GpkgEntry::File(b"meta")),
        ("pkg-1.0/Manifest", GpkgEntry::Symlink("/etc/hostname")),
    ]);
    assert_rejects_everywhere(&g, &["Manifest", "a symbolic link"]);
}

/// A symlinked `metadata.tar.zst`: all three sites reject it by type.
#[test]
fn regular_outer_member_check_rejects_a_symlinked_metadata_member() {
    let (_guard, g) = build_gpkg_entries(&[
        ("pkg-1.0/gpkg-1", GpkgEntry::File(b"")),
        (
            "pkg-1.0/metadata.tar.zst",
            GpkgEntry::Symlink("/etc/hostname"),
        ),
        (
            "pkg-1.0/Manifest",
            GpkgEntry::File(b"DATA gpkg-1 0 BLAKE2B x SHA512 y\n"),
        ),
    ]);
    assert_rejects_everywhere(&g, &["metadata.tar.zst", "a symbolic link"]);
}

/// A symlinked prefix: the prefix name itself is a top-level member
/// (zero slashes), which the structure check refuses before any type
/// check.
#[test]
fn regular_outer_member_check_rejects_a_symlinked_prefix_directory() {
    let (_guard, g) = build_gpkg_entries(&[
        ("pkg-1.0", GpkgEntry::Symlink("/etc")),
        ("pkg-1.0-members/gpkg-1", GpkgEntry::File(b"")),
        ("pkg-1.0-members/metadata.tar.zst", GpkgEntry::File(b"meta")),
    ]);
    assert_rejects_everywhere(&g, &["pkg-1.0", "gpkg file structure mismatch"]);
}

/// A FIFO at `metadata.tar.zst`: rejected by type before any read (so
/// the test returns rather than hangs).
#[test]
fn regular_outer_member_check_rejects_a_fifo_metadata_member() {
    let (_guard, g) = build_gpkg_entries(&[
        ("pkg-1.0/gpkg-1", GpkgEntry::File(b"")),
        ("pkg-1.0/metadata.tar.zst", GpkgEntry::Fifo),
        (
            "pkg-1.0/Manifest",
            GpkgEntry::File(b"DATA gpkg-1 0 BLAKE2B x SHA512 y\n"),
        ),
    ]);
    assert_rejects_everywhere(&g, &["metadata.tar.zst", "a FIFO"]);
}

/// A hardlink to another member extracts as a regular file, so the
/// type check passes and the reading paths reject it instead.
#[test]
fn regular_outer_member_check_still_rejects_a_hardlinked_metadata_member() {
    let manifest = format!(
        "{}DATA metadata.tar.zst 471 BLAKE2B x SHA512 y\n",
        data_line("gpkg-1", b"")
    );
    let (_guard, g) = build_gpkg_entries(&[
        ("pkg-1.0/gpkg-1", GpkgEntry::File(b"")),
        (
            "pkg-1.0/metadata.tar.zst",
            GpkgEntry::Hardlink("pkg-1.0/gpkg-1"),
        ),
        ("pkg-1.0/Manifest", GpkgEntry::File(manifest.as_bytes())),
    ]);
    assert_rejects_everywhere(&g, &["metadata.tar.zst"]);
}

/// A symlinked `gpkg-1` marker.
#[test]
fn regular_outer_member_check_rejects_a_symlinked_gpkg_marker() {
    let (_guard, g) = build_gpkg_entries(&[
        ("pkg-1.0/gpkg-1", GpkgEntry::Symlink("Manifest")),
        ("pkg-1.0/metadata.tar.zst", GpkgEntry::File(b"meta")),
        (
            "pkg-1.0/Manifest",
            GpkgEntry::File(b"DATA gpkg-1 0 BLAKE2B x SHA512 y\n"),
        ),
    ]);
    assert_rejects_everywhere(&g, &["gpkg-1", "a symbolic link"]);
}

/// A character device at `image.tar.zst` (root only). All three sites
/// reject it by type before any read.
#[test]
fn regular_outer_member_check_rejects_a_char_device_image_member_when_root() {
    // SAFETY: `geteuid` takes no arguments and returns a plain int.
    if unsafe { libc::geteuid() } != 0 {
        eprintln!(
            "skipping regular_outer_member_check_rejects_a_char_device_image_member_when_root: not root"
        );
        return;
    }
    let (_guard, g) = build_gpkg_entries(&[
        ("pkg-1.0/gpkg-1", GpkgEntry::File(b"")),
        ("pkg-1.0/metadata.tar.zst", GpkgEntry::File(b"meta")),
        (
            "pkg-1.0/image.tar.zst",
            GpkgEntry::CharDevice { major: 1, minor: 5 },
        ),
        (
            "pkg-1.0/Manifest",
            GpkgEntry::File(b"DATA gpkg-1 0 BLAKE2B x SHA512 y\n"),
        ),
    ]);
    assert_rejects_everywhere(&g, &["image.tar.zst", "a character device"]);
}
