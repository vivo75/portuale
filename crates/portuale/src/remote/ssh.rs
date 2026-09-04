//! The ssh transport: argv, control sockets, running scripts, copying files.

use crate::remote::*;

// --- Shell quoting ---------------------------------------------------------

/// Quote one argv word for `bash -s` payloads: single-quote, escaping
/// embedded quotes as `'\''`. Client paths are never interpolated raw.
pub fn sh_quote(word: &str) -> String {
    let mut out = String::with_capacity(word.len() + 2);
    out.push('\'');
    for chunk in word.split('\'') {
        // Rejoin with the close-escape-reopen sequence; the first split
        // boundary needs no opener (already inside quotes).
        if out.len() > 1 {
            out.push_str("'\\''");
        }
        out.push_str(chunk);
    }
    out.push('\'');
    out
}

/// Worst-case `%C` expansion plus ssh's own multiplex suffix, with
/// margin under the 108-byte `sun_path` cap.
pub(crate) const CONTROL_PATH_BUDGET: usize = 64;

pub(crate) fn control_dir_for(
    xdg_runtime: Option<std::path::PathBuf>,
    home: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Some(xdg) = xdg_runtime {
        candidates.push(xdg.join(".portuale-cp"));
    }
    if let Some(home) = home {
        candidates.push(home.join(".portuale/cp"));
    }
    candidates.into_iter().find(|dir| {
        if dir.to_string_lossy().len() + CONTROL_PATH_BUDGET >= 108 {
            return false;
        }
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        true
    })
}

/// The local transport's `bash -s`: the child would inherit the
/// server process's environment, so the server's own VDB selection
/// (the `PORTUALE_VDB_*` vars and the VDB IPC var) is removed and
/// `PORTUALE_BIN` is set. Only this transport needs it; over ssh
/// nothing is forwarded.
pub(crate) fn local_client_bash() -> std::process::Command {
    let mut cmd = std::process::Command::new("bash");
    cmd.arg("-s");
    for var in [
        "PORTUALE_VDB_BACKEND",
        "PORTUALE_VDB_PATH",
        "PORTUALE_VDB_ROOT",
        crate::vdb_ipc::IPC_VAR,
    ] {
        cmd.env_remove(var);
    }
    cmd.env("PORTUALE_BIN", crate::self_exe::self_exe());
    cmd
}
