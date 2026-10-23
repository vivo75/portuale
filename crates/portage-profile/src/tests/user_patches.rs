//! Backlog #509: local user-patch digest, Portage `UserPatches`
//! (`lib/portage/package/ebuild/_config/UserPatches.py`) and the shell
//! `eapply_user` writer (`bin/phase-helpers.sh`) agree on the algorithm:
//! per-file sha256 hex of the patch files matching the package, basenames
//! sorted LC_ALL=C, empty files skipped, sha256 over the concatenated hex.
//! Oracle: host `userPatchDigest` values in
//! `LLM/evidence/2026-10-23-509/`; upstream NIST vectors in
//! `MockUserPatches` (`Patch1`+`Patch2` ->
//! `05f0b2d539792008c12ef9993a5d27ab85837c2b34317e423b0fd9322a9d6918`,
//! `Patch3`+`Patch4` ->
//! `506f0b5b42ef2d861c00fe67aba3f33889a2b433febc0291f9d237acd3e0b4f8`).

use std::collections::HashSet;
use std::path::Path;

use crate::UserPatchSet;
use portage_util::TempDir;

fn write_patch(dir: &Path, name: &str, content: &[u8]) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(name), content).unwrap();
}

// Upstream `MockUserPatches` byte vectors (NIST test vectors).
const PATCH1: &[u8] = b"\xd3";
const PATCH2: &[u8] = b"\x11\xaf";
const PATCH3: &[u8] = b"\xb4\x19\x0e";
const PATCH4: &[u8] = b"\x74\xba\x25\x21";

fn populated_root() -> std::path::PathBuf {
    let tmp = TempDir::new("portage-profile-test-user-patches").keep();
    let foo = tmp.join("etc/portage/patches/app-misc/foo");
    write_patch(&foo, "user.patch", PATCH1);
    write_patch(&foo, "random.diff", PATCH2);
    let bar = tmp.join("etc/portage/patches/app-misc/bar");
    write_patch(&bar, "user.patch", PATCH3);
    write_patch(&bar, "random.diff", PATCH4);
    // Empty files negate (shell) / hash-skipped (python): no digest half.
    write_patch(
        &tmp.join("etc/portage/patches/app-misc/neg"),
        "user.patch",
        b"",
    );
    tmp
}

/// sha256 hex of one file's content, then sha256 over that hex string:
/// the digest of a single matching patch file.
fn single_file_digest(content: &[u8]) -> String {
    use sha2::Digest as _;
    let inner = format!("{:x}", sha2::Sha256::digest(content));
    format!("{:x}", sha2::Sha256::digest(inner.as_bytes()))
}

#[test]
fn digest_matches_upstream_mock_vectors() {
    let tmp = populated_root();
    let set = UserPatchSet::load(&tmp).expect("patches dir exists");
    assert_eq!(
        set.digest("app-misc", "foo", "1.0", "0"),
        "05f0b2d539792008c12ef9993a5d27ab85837c2b34317e423b0fd9322a9d6918"
    );
    assert_eq!(
        set.digest("app-misc", "bar", "1.0", "0"),
        "506f0b5b42ef2d861c00fe67aba3f33889a2b433febc0291f9d237acd3e0b4f8"
    );
}

#[test]
fn empty_patch_file_contributes_no_digest_but_lists_the_file() {
    let tmp = populated_root();
    let set = UserPatchSet::load(&tmp).expect("patches dir exists");
    // Only an empty file: present (like `in UserPatches`) but digest "".
    assert_eq!(set.digest("app-misc", "neg", "1.0", "0"), "");
    assert_eq!(
        set.files("app-misc", "neg", "1.0", "0"),
        vec!["app-misc/neg/user.patch".to_string()]
    );
    // Nothing at all: "" digest, no files.
    assert_eq!(set.digest("app-misc", "absent", "1.0", "0"), "");
    assert!(set.files("app-misc", "absent", "1.0", "0").is_empty());
}

#[test]
fn more_specific_dir_overrides_the_basename() {
    let tmp = populated_root();
    // `cat/pkg-ver` overrides `cat/pkg` for the same basename.
    write_patch(
        &tmp.join("etc/portage/patches/app-misc/foo-1.0"),
        "user.patch",
        PATCH3,
    );
    let set = UserPatchSet::load(&tmp).expect("patches dir exists");
    // digest over {random.diff: P2, user.patch: P3}, not {P1, P2}.
    assert_ne!(
        set.digest("app-misc", "foo", "1.0", "0"),
        "05f0b2d539792008c12ef9993a5d27ab85837c2b34317e423b0fd9322a9d6918"
    );
    assert_eq!(
        set.files("app-misc", "foo", "1.0", "0")
            .into_iter()
            .collect::<HashSet<_>>(),
        HashSet::from([
            "app-misc/foo/user.patch".to_string(),
            "app-misc/foo/random.diff".to_string(),
            "app-misc/foo-1.0/user.patch".to_string(),
        ])
    );
}

#[test]
fn slot_qualified_dirs_participate() {
    let tmp = populated_root();
    write_patch(
        &tmp.join("etc/portage/patches/app-misc/slotted:2"),
        "user.patch",
        PATCH1,
    );
    let set = UserPatchSet::load(&tmp).expect("patches dir exists");
    assert_eq!(
        set.digest("app-misc", "slotted", "1.0", "2"),
        single_file_digest(PATCH1)
    );
    assert_eq!(set.digest("app-misc", "slotted", "1.0", "0"), "");
}

#[test]
fn stray_files_outside_pkg_dirs_are_ignored() {
    // Portage 7964f58e2 (bug 982999): a `patches` file, or stray files
    // directly under `patches/` or `patches/<cat>/`, are ignored, not a
    // crash. Oracle: `LLM/evidence/2026-10-23-509/captures/15-stray-k.*`.
    let tmp = populated_root();
    std::fs::write(tmp.join("etc/portage/patches/stray.patch"), b"x").unwrap();
    let stray_cat = tmp.join("etc/portage/patches/app-misc-stray");
    write_patch(&stray_cat, "user.patch", b"x");
    let set = UserPatchSet::load(&tmp).expect("patches dir exists");
    assert_eq!(
        set.digest("app-misc", "foo", "1.0", "0"),
        "05f0b2d539792008c12ef9993a5d27ab85837c2b34317e423b0fd9322a9d6918"
    );
    // `patches` itself a file: no patch sets at all.
    let tmp2 = TempDir::new("portage-profile-test-user-patches-file").keep();
    std::fs::create_dir_all(tmp2.join("etc/portage")).unwrap();
    std::fs::write(tmp2.join("etc/portage/patches"), b"x").unwrap();
    assert!(UserPatchSet::load(&tmp2).is_none());
    // Missing entirely: none as well.
    let tmp3 = TempDir::new("portage-profile-test-user-patches-missing").keep();
    assert!(UserPatchSet::load(&tmp3).is_none());
}
