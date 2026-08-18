// Portage: data.py. Merging needs `uid == 0`, except a non-root caller who owns
// the target root (unprivileged mode). The `portage`-group middle tier is
// collapsed away: every merge/unmerge needs the top tier.

use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    fn tempdir() -> std::path::PathBuf {
        TempDir::new("portuale-privileges-test").keep()
    }
}
