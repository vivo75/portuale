//! `NEEDED.ELF.2` parsing and the linkage map live in
//! [`portage_repo::needed_elf`]; this module adds the one piece that needs the
//! binary's `fnmatch`.

pub use portage_repo::needed_elf::*;

/// [`portage_repo::needed_elf::generate_soname_deps`] with the binary's
/// `fnmatch` for `PROVIDES_EXCLUDE` / `REQUIRES_EXCLUDE`.
// Portage: soname_deps.py, SonameDepsProcessor
pub fn generate_soname_deps(
    entries: &[NeededEntry],
    provides_exclude: &str,
    requires_exclude: &str,
) -> (Option<String>, Option<String>) {
    portage_repo::needed_elf::generate_soname_deps(
        entries,
        provides_exclude,
        requires_exclude,
        &crate::install_mask::fnmatch,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SonameDepsProcessor` output: provides become `PROVIDES`, needed
    /// sonames become `REQUIRES` unless the package itself provides them, and
    /// the exclude patterns filter both.
    #[test]
    fn generate_soname_deps_matches_the_portage_soname_deps_processor() {
        let entries = vec![
            NeededEntry::parse("X86_64;/usr/bin/x;;;libc.so.6;x86_64").unwrap(),
            NeededEntry::parse("X86_64;/usr/lib64/libfoo.so.1;libfoo.so.1;;;x86_64").unwrap(),
            // Consumes the soname it also provides: must not land in REQUIRES.
            NeededEntry::parse("X86_64;/usr/lib64/libfoo.so.1;libfoo.so.1;;libfoo.so.1;x86_64")
                .unwrap(),
        ];
        let (provides, requires) = generate_soname_deps(&entries, "", "");
        assert_eq!(provides.as_deref(), Some("x86_64: libfoo.so.1\n"));
        assert_eq!(requires.as_deref(), Some("x86_64: libc.so.6\n"));

        // `PROVIDES_EXCLUDE`/`REQUIRES_EXCLUDE` globs.
        let (provides, requires) = generate_soname_deps(&entries, "libfoo.so*", "libc.so*");
        assert_eq!(provides, None);
        assert_eq!(requires, None);

        // An entry with no multilib category is the caller's own
        // "unrecognized ELF" case and contributes nothing.
        let no_cat = vec![NeededEntry::parse("X86_64;/usr/bin/x;;;libc.so.6;").unwrap()];
        assert_eq!(generate_soname_deps(&no_cat, "", ""), (None, None));
    }
}
