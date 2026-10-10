// Portage: data.py. Merging needs `uid == 0`, except a non-root caller who owns
// the target root (unprivileged mode). The `portage`-group middle tier is
// collapsed away: every merge/unmerge needs the top tier.

use std::path::Path;

/// May this process merge/unmerge into `root`? True for uid 0, or when a
/// non-root caller owns `root`. A missing `root` checks the nearest existing
/// ancestor.
/// Portage: data.py, secpass.
pub fn is_privileged(root: &Path) -> bool {
    // SAFETY: `geteuid` takes no arguments and returns a plain uid.
    if unsafe { libc::geteuid() } == 0 {
        return true;
    }

    let mut probe = root;
    let existing = loop {
        if probe.exists() {
            break Some(probe);
        }
        match probe.parent() {
            Some(parent) => probe = parent,
            None => break None,
        }
    };
    let Some(dir) = existing else {
        return false;
    };

    let Ok(meta) = std::fs::metadata(dir) else {
        return false;
    };
    use std::os::unix::fs::PermissionsExt;
    if meta.permissions().mode() & 0o0002 != 0 {
        // World-writable alone is not ownership.
        return false;
    }

    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a NUL-terminated path that outlives the call; the
    // second argument is a plain access-mode flag.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// Print the superuser error plus the `--pretend` hint. `prog` is `"emerge"` or
/// `"ebuild"`.
/// Portage: actions.py.
pub fn deny_superuser(prog: &str) {
    eprintln!("{prog}: superuser access is required");
    eprintln!("{prog}: (add --pretend to preview without merging)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    fn tempdir() -> std::path::PathBuf {
        TempDir::new("portuale-privileges-test").keep()
    }

    #[test]
    fn root_is_always_privileged() {
        // SAFETY: `geteuid` takes no arguments.
        if unsafe { libc::geteuid() } == 0 {
            assert!(is_privileged(std::path::Path::new("/usr/nonexistent-xyz")));
        }
    }

    #[test]
    fn a_dir_the_caller_owns_is_unprivileged_mode() {
        let dir = tempdir();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_privileged(&dir));
        assert!(is_privileged(&dir.join("root/deeper")));
    }

    #[test]
    fn a_world_writable_dir_is_not_ownership() {
        // SAFETY: `geteuid` takes no arguments.
        if unsafe { libc::geteuid() } == 0 {
            return; // root passes the early return before the mode check
        }
        let dir = tempdir();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(!is_privileged(&dir));
    }

    #[test]
    fn a_dir_the_caller_cannot_write_is_not_privileged() {
        // SAFETY: `geteuid` takes no arguments.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        // /usr exists everywhere, is root-owned and not world-writable.
        assert!(!is_privileged(std::path::Path::new(
            "/usr/portuale-privileges-test-xyz"
        )));
    }
}
