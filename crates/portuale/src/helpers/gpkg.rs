// Native `gpkg-helper.py compress`: builds a gpkg binpkg from two dirs.
//
// A port of `command_compose` over `gpkg.compress`: metadata dict first,
// then the image pre-scan, then the container (`gpkg-1`, compressor
// resolved after the container exists, metadata and image members with
// `.sig`s, `Manifest`, close). Tar headers are hand-rendered USTAR,
// GNU past the 100/155/8000000000 thresholds. It writes what the
// `binpkg.rs` reader reads.
// Portage: bin/gpkg-helper.py, command_compose

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};

use blake2::Blake2b512;
use sha2::{Digest, Sha512};

/// `gpkg.checksums` and `_record_checksum`.
#[derive(Default)]
struct Checksums {
    records: Vec<Vec<String>>,
}

// ---------------------------------------------------------------------
// The container and the stream writer
// ---------------------------------------------------------------------

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Clone, Copy)]
enum NameKind {
    User,
    Group,
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;
    use std::os::unix::fs::PermissionsExt;

    fn unsigned_verify() -> crate::binpkg::GpgVerify {
        crate::binpkg::GpgVerify {
            verify_signature: true,
            request_signature: false,
            base_command: crate::binpkg::DEFAULT_GPG_VERIFY_BASE_COMMAND.to_string(),
            gpg_home: "/nonexistent-gnupg".to_string(),
        }
    }
}
