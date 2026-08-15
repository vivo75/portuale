// The `$PKGDIR` directory-scan fallback (Portage: `bintree._populate_local`).
// Every other binary-package path here is `<pkgdir>/Packages`-index driven,
// so a `gpkg`/`xpak` *listed in an index* already resolves. When `$PKGDIR`
// holds binpkg *files* but no `Packages`, each file's own embedded metadata
// is read instead, with a per-format reader (`read_gpkg_metadata` for
// `.gpkg.tar`, `read_xpak_metadata` for `.tbz2`/`.xpak`). The scan output
// becomes `portage_profile::Config::scanned_binpkgs`, never written back
// to `Packages`.

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use std::fs;

use std::io::Read;

use std::path::{Path, PathBuf};

use std::process::{Command, Stdio};
