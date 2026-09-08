// Real `portage.data`'s own security-level model (`lib/portage/data.py:
// 93-166`): install/uninstall operations that write into a root-owned
// filesystem need `uid == 0`. The one documented exception is real
// "unprivileged mode" (`_unprivileged_mode`, `data.py:111-114`): a
// non-root user who *owns* the target root -- can write to it, and it is
// not merely world-writable -- may still merge into it (real `secpass`
// becomes 2, using the root dir's own uid/gid for installed files). That
// covers a prefix / chroot / staging tree the user set up themselves.
//
// Portage additionally has a middle tier (`secpass == 1`, membership in
// the `portage` group) that only grants `--fetchonly` / `--buildpkgonly`;
// portuale has no `portage`-group concept, so this collapses to a plain
// "privileged or not" check -- matching real portage's own behaviour for
// every real merge/unmerge action, which always requires `secpass == 2`.

use std::path::Path;

/// Real `uid == 0 or _unprivileged_mode(eroot, eroot_st)` (real `secpass
/// == 2`): may this process merge/unmerge into `root`?
///
/// `true` when the effective uid is 0, or when a non-root caller owns
/// `root` (writable, and not writable only because of the world-writable
/// bit -- real `not eroot_st.st_mode & 0o0002`). When `root` does not
/// exist yet, the nearest existing ancestor is checked, matching real
/// `first_existing`.
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
        // Writable only because it is world-writable -- real portage does
        // not treat this as ownership.
        return false;
    }

    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a NUL-terminated path that outlives the call; the
    // second argument is a plain access-mode flag.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// Real `actions.py:3965-4012`'s own error path, narrowed to portuale's
/// single privilege tier: print `<prog>: superuser access is required`
/// and the hint that `--pretend` previews without it. `prog` is
/// `"emerge"` or `"ebuild"`.
pub fn deny_superuser(prog: &str) {
    eprintln!("{prog}: superuser access is required");
    eprintln!("{prog}: (add --pretend to preview without merging)");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "portuale-privileges-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
        // A tmp dir this process just created -- owned, writable, not
        // world-writable after the explicit chmod.
        let dir = tempdir();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_privileged(&dir));
        // A not-yet-created child resolves to the owned parent.
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
