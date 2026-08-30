use super::*;

#[test]
fn control_dir_prefers_short_dirs_and_skips_unfitting_ones() {
    use std::path::PathBuf;
    // A deep base cannot fit `%C` + suffix under 108 bytes.
    let deep = PathBuf::from("/tmp/pytest-of-vivo/pytest-707/loopback-sshd0/home");
    assert!(control_dir_for(None, Some(deep)).is_none());
    // XDG_RUNTIME_DIR wins when it fits (and gets created 0700).
    // Deliberately NOT `TempDir`: this test is about the 108-byte
    // `sun_path` budget, and `TempDir`'s own `portuale-td-…-{pid}-{nanos}`
    // name already eats too much of it to fit `%C` + suffix.
    let tmp = PathBuf::from("/tmp/cpfit");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let picked = control_dir_for(Some(tmp.clone()), None);
    assert_eq!(picked, Some(tmp.join(".portuale-cp")));
    assert!(tmp.join(".portuale-cp").is_dir());
    let _ = std::fs::remove_dir_all(&tmp);
    // Nothing configured at all: no multiplexing.
    assert!(control_dir_for(None, None).is_none());
}
