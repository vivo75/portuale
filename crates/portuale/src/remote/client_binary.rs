//! Finding, checking and installing portuale on the client.

use crate::remote::*;

// --- Client portuale binary ----------------------------------------------------

/// Default client search directories, in D5 order.
pub(crate) fn default_bin_dirs() -> Vec<String> {
    vec!["/opt/bin".to_string(), "/usr/local/bin".to_string()]
}

/// Candidate directories: only the `--remote-portuale-dir` pair
/// when that option is given, else the default pair.
pub(crate) fn bin_search_dirs(ctx: &RemoteContext) -> Vec<String> {
    match &ctx.portuale_dir {
        Some(dir) => vec![dir.clone()],
        None => default_bin_dirs(),
    }
}

/// Candidate files in check order: per directory, `portuale` then
/// `portuale-<hash>`. The first file whose full digest matches is used.
pub(crate) fn bin_candidates(dirs: &[String], short_hash: &str) -> Vec<String> {
    let mut out = Vec::new();
    for dir in dirs {
        out.push(format!("{dir}/portuale"));
        out.push(format!("{dir}/portuale-{short_hash}"));
    }
    out
}

/// The server-side identity of the binary to ship.
pub(crate) struct ServerBinInfo {
    /// The file to stream (`--remote-portuale-binary`, or `current_exe()`).
    pub path: std::path::PathBuf,
    /// Full SHA-256 hex of the file.
    pub digest_hex: String,
    /// First 16 hex digits of the digest (the `<hash>` in file names).
    pub short_hash: String,
    /// ELF `e_machine` of the file.
    pub machine: u16,
}

/// Read the shipped binary's ELF `e_machine`: bytes 18-19, in the
/// endianness byte 5 names. Header bytes are read directly; no new crate.
pub(crate) fn elf_machine(bytes: &[u8]) -> Result<u16, String> {
    if bytes.len() < 20 {
        return Err("not an ELF binary (shorter than the 20-byte header prefix)".to_string());
    }
    if &bytes[0..4] != b"\x7fELF" {
        return Err("not an ELF binary (bad magic)".to_string());
    }
    match bytes[5] {
        1 => Ok(u16::from_le_bytes([bytes[18], bytes[19]])),
        2 => Ok(u16::from_be_bytes([bytes[18], bytes[19]])),
        other => Err(format!("unknown ELF data encoding {other}")),
    }
}

/// Hash the binary the server will ship and read its ELF machine.
/// Errors name `--remote-portuale-binary` when that option selected
/// the file.
pub(crate) fn server_bin_info(ctx: &RemoteContext) -> Result<ServerBinInfo, String> {
    let (path, named) = match &ctx.portuale_binary {
        Some(custom) => (std::path::PathBuf::from(custom), true),
        None => (crate::self_exe::self_exe().to_path_buf(), false),
    };
    let bytes = std::fs::read(&path).map_err(|e| {
        if named {
            format!("mrg: --remote-portuale-binary {}: {e}", path.display())
        } else {
            format!(
                "mrg: cannot read the running binary {} for the client install: {e}",
                path.display()
            )
        }
    })?;
    use sha2::Digest as _;
    let digest_hex = format!("{:x}", sha2::Sha256::digest(&bytes));
    let machine = elf_machine(&bytes).map_err(|e| {
        if named {
            format!("mrg: --remote-portuale-binary {}: {e}", path.display())
        } else {
            format!("mrg: the running binary {}: {e}", path.display())
        }
    })?;
    Ok(ServerBinInfo {
        short_hash: digest_hex[..16].to_string(),
        digest_hex,
        machine,
        path,
    })
}

/// Server-side preflight facts for the binary gates: what to look for
/// and what to compare against. `None` for local transport without
/// `--remote-portuale-dir`, which keeps `current_exe()` and installs
/// nothing.
pub(crate) struct BinPreflight {
    /// `bin_search_dirs(ctx)`.
    pub dirs: Vec<String>,
    /// `bin_candidates(&dirs, &short)`, in D5 order.
    pub candidates: Vec<String>,
    /// Full expected SHA-256 hex.
    pub digest: String,
    /// The file the digest (and bytes) come from, for messages.
    pub bin_path: String,
    /// ELF `e_machine` of that file.
    pub machine: u16,
}

/// Compute the binary preflight facts, unless this run skips the gates
/// by design (local transport without the override).
pub(crate) fn bin_preflight(ctx: &RemoteContext) -> Result<Option<BinPreflight>, String> {
    if ctx.transport == RemoteTransport::Local && ctx.portuale_dir.is_none() {
        return Ok(None);
    }
    let info = server_bin_info(ctx)?;
    let dirs = bin_search_dirs(ctx);
    let candidates = bin_candidates(&dirs, &info.short_hash);
    Ok(Some(BinPreflight {
        dirs,
        candidates,
        digest: info.digest_hex,
        bin_path: info.path.display().to_string(),
        machine: info.machine,
    }))
}
