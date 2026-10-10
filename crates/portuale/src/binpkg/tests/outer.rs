use super::*;

/// `#56` S0 cell a: a symlinked `Manifest` was followed onto the host
/// (`fs::read` read `/etc/hostname`); all three sites now reject it by
/// type, before any read.
#[test]
fn regular_outer_member_check_rejects_a_symlinked_manifest() {
    let g = build_gpkg_entries(&[
        ("pkg-1.0/gpkg-1", GpkgEntry::File(b"")),
        ("pkg-1.0/metadata.tar.zst", GpkgEntry::File(b"meta")),
        ("pkg-1.0/Manifest", GpkgEntry::Symlink("/etc/hostname")),
    ]);
    assert_rejects_everywhere(&g, &["Manifest", "a symbolic link"]);
}

/// `#56` S0 cell b: a symlinked `metadata.tar.zst` was followed by the
/// decompressor; all three sites now reject it by type.
#[test]
fn regular_outer_member_check_rejects_a_symlinked_metadata_member() {
    let g = build_gpkg_entries(&[
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

/// `#56` S0 cell c: a symlinked prefix made the walk list a host
/// directory (`/etc`, 266 dirents). `#58` S5 rejects the crafted
/// member from its header instead -- the prefix name itself is a
/// top-level member (`pkg-1.0`, zero slashes), which real's own
/// `f.count("/") != 1` structure check refuses before any type
/// check, so all three sites reject it the same way.
#[test]
fn regular_outer_member_check_rejects_a_symlinked_prefix_directory() {
    let g = build_gpkg_entries(&[
        ("pkg-1.0", GpkgEntry::Symlink("/etc")),
        ("pkg-1.0-members/gpkg-1", GpkgEntry::File(b"")),
        ("pkg-1.0-members/metadata.tar.zst", GpkgEntry::File(b"meta")),
    ]);
    assert_rejects_everywhere(&g, &["pkg-1.0", "gpkg file structure mismatch"]);
}

/// `#56` S0 cell d: a FIFO at `metadata.tar.zst` blocked the
/// decompressor forever (which is why the check runs before any
/// read); all three sites now reject it by type, so the test must
/// return rather than hang.
#[test]
fn regular_outer_member_check_rejects_a_fifo_metadata_member() {
    let g = build_gpkg_entries(&[
        ("pkg-1.0/gpkg-1", GpkgEntry::File(b"")),
        ("pkg-1.0/metadata.tar.zst", GpkgEntry::Fifo),
        (
            "pkg-1.0/Manifest",
            GpkgEntry::File(b"DATA gpkg-1 0 BLAKE2B x SHA512 y\n"),
        ),
    ]);
    assert_rejects_everywhere(&g, &["metadata.tar.zst", "a FIFO"]);
}

/// `#56` S0 cell f: a hardlink to another member extracts as a
/// *regular* file (the walk cannot see the header type), so the
/// type check passes and the pre-existing reading paths reject it --
/// the decompressor on the 0-byte link and the Manifest size check.
/// No host bytes are read (S0's own verdict).
#[test]
fn regular_outer_member_check_still_rejects_a_hardlinked_metadata_member() {
    let manifest = format!(
        "{}DATA metadata.tar.zst 471 BLAKE2B x SHA512 y\n",
        data_line("gpkg-1", b"")
    );
    let g = build_gpkg_entries(&[
        ("pkg-1.0/gpkg-1", GpkgEntry::File(b"")),
        (
            "pkg-1.0/metadata.tar.zst",
            GpkgEntry::Hardlink("pkg-1.0/gpkg-1"),
        ),
        ("pkg-1.0/Manifest", GpkgEntry::File(manifest.as_bytes())),
    ]);
    assert_rejects_everywhere(&g, &["metadata.tar.zst"]);
}

/// `#56` S0 cell h: a symlinked `gpkg-1` marker was trusted by name
/// and, on the merge path, read `Manifest` through the link.
#[test]
fn regular_outer_member_check_rejects_a_symlinked_gpkg_marker() {
    let g = build_gpkg_entries(&[
        ("pkg-1.0/gpkg-1", GpkgEntry::Symlink("Manifest")),
        ("pkg-1.0/metadata.tar.zst", GpkgEntry::File(b"meta")),
        (
            "pkg-1.0/Manifest",
            GpkgEntry::File(b"DATA gpkg-1 0 BLAKE2B x SHA512 y\n"),
        ),
    ]);
    assert_rejects_everywhere(&g, &["gpkg-1", "a symbolic link"]);
}

/// `#56` S0 cell e (root only, like the device itself): a character
/// device at `image.tar.zst` made the merge path read `/dev/zero`
/// unboundedly. All three sites reject it by type before any read.
#[test]
fn regular_outer_member_check_rejects_a_char_device_image_member_when_root() {
    // SAFETY: `geteuid` takes no arguments and returns a plain int.
    if unsafe { libc::geteuid() } != 0 {
        eprintln!(
            "skipping regular_outer_member_check_rejects_a_char_device_image_member_when_root: not root"
        );
        return;
    }
    let g = build_gpkg_entries(&[
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
