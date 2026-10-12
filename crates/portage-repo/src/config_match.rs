//! Per-package index over `package.*` config atoms.

use crate::*;

/// Whether `entry` (a `package.mask`/`.unmask`/`.accept_keywords` line)
/// matches this candidate: `match_from_list`, which re-parses with
/// `allow_wildcard: true, allow_repo: Some(true)` like Portage (dep:2901-2906).
pub(crate) fn matches_config_entry(
    entry: &str,
    candidate_str: &str,
    _category: &str,
    _package: &str,
) -> bool {
    matches_config_entry_with_build_id(entry, candidate_str, None, _category, _package)
}

/// [`matches_config_entry`] for a candidate carrying a binary build-id
/// (Portage `_pkg_str.build_id`): an `=`+build-id entry matches only on
/// equal ids, every other operator ignores it (dep:2994-3008, S9).
pub(crate) fn matches_config_entry_with_build_id(
    entry: &str,
    candidate_str: &str,
    build_id: Option<&str>,
    _category: &str,
    _package: &str,
) -> bool {
    portage_dep::match_from_list_with_build_id(entry, &[(candidate_str, build_id)])
        .is_some_and(|m| !m.is_empty())
}

/// Whether a command-line option atom (`--exclude`, `--reinstall-atoms`,
/// `--rebuild-exclude`/`--rebuild-ignore`, `--usepkg-exclude`/`--usepkg-include`,
/// `--useoldpkg-atoms`, `--buildpkg-exclude`, `--getbinpkg-exclude`/`--getbinpkg-include`)
/// matches this candidate: like [`matches_config_entry`], plus Portage's
/// `WildcardPackageSet._implicitWildcarding`
/// (`lib/portage/_sets/base.py`): a category-less entry is retried as
/// `*/entry`, so `--exclude t341-dep` excludes every category's
/// `t341-dep`. Config files (`package.*`) never do this — a
/// category-less line there is invalid (S5/S6) — so they keep using
/// [`matches_config_entry`] directly.
pub(crate) fn matches_cli_option_atom(entry: &str, candidate_str: &str) -> bool {
    if portage_dep::match_from_list(entry, &[candidate_str]).is_some_and(|m| !m.is_empty()) {
        return true;
    }
    // Portage tries the plain `Atom` first and prefixes `*/` only on
    // `InvalidAtom`: an entry that already carries a category never falls
    // through to the wildcard form.
    if entry.split(':').next().unwrap_or(entry).contains('/') {
        return false;
    }
    let prefixed = format!("*/{entry}");
    portage_dep::match_from_list(&prefixed, &[candidate_str]).is_some_and(|m| !m.is_empty())
}

/// One `package.*` config list's atoms bucketed by `cat/pkg` -- the stand-in
/// for Portage's `ExtendedAtomDict` (`lib/portage/dep/__init__.py`). A flat
/// scan was ~half of the post-`parse_atom`-cache run; this narrows it to a
/// `HashMap` lookup plus the wildcard tail.
#[derive(Default)]
pub(crate) struct CpBucketIndex {
    /// `"cat/pkg"` -> positions (in original file/stacking order) of every
    /// plain-atom entry for that exact `cp`.
    pub(crate) by_cp: HashMap<String, Vec<u32>>,
    /// Positions of every entry whose atom is a bounded wildcard or didn't
    /// parse -- always re-checked, in file order. Small on a Portage tree.
    pub(crate) other: Vec<u32>,
}

impl CpBucketIndex {
    pub(crate) fn build<T: ConfigAtomEntry>(entries: &[T]) -> Self {
        let mut by_cp: HashMap<String, Vec<u32>> = HashMap::new();
        let mut other = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            // `parse_atom` handles a leading `!`/`!!` blocker itself; the
            // `package.*` lists carry none, but `--exclude` reuses this.
            match portage_dep::parse_atom(e.config_atom()) {
                Some(atom) => by_cp
                    .entry(format!("{}/{}", atom.category, atom.package))
                    .or_default()
                    .push(i as u32),
                None => other.push(i as u32),
            }
        }
        CpBucketIndex { by_cp, other }
    }

    /// Positions of every entry that could match `category/package`, in
    /// original file order (`by_cp` hit ∪ `other`, sorted).
    pub(crate) fn candidate_positions(&self, category: &str, package: &str) -> Vec<u32> {
        let mut positions = match self.by_cp.get(&format!("{category}/{package}")) {
            Some(v) => v.clone(),
            None => Vec::new(),
        };
        positions.extend_from_slice(&self.other);
        positions.sort_unstable();
        positions
    }
}

/// An entry in a `package.*` config list, exposing just its atom string.
pub(crate) trait ConfigAtomEntry {
    fn config_atom(&self) -> &str;
}

impl ConfigAtomEntry for (String, Vec<String>) {
    fn config_atom(&self) -> &str {
        &self.0
    }
}

impl ConfigAtomEntry for String {
    fn config_atom(&self) -> &str {
        self.as_str()
    }
}

/// One memoised [`CpBucketIndex`] plus its first/last-atom fingerprint (see
/// [`cp_bucket_index`]).
pub(crate) struct CachedBucketIndex {
    pub(crate) first_atom: String,
    pub(crate) last_atom: String,
    pub(crate) index: Rc<CpBucketIndex>,
}

/// The [`CpBucketIndex`] for one config list, built once and memoised for
/// the life of the process. Keyed by backing-storage identity with the first
/// and last atoms as a fingerprint against freed-then-reused allocations.
/// Thread-local so the lookup stays lock-free, like the `parse_atom` cache.
pub(crate) fn cp_bucket_index<T: ConfigAtomEntry>(entries: &[T]) -> Rc<CpBucketIndex> {
    thread_local! {
        static EMPTY: Rc<CpBucketIndex> = Rc::new(CpBucketIndex::default());
        static CACHE: RefCell<HashMap<(usize, usize), CachedBucketIndex>> =
            RefCell::new(HashMap::new());
    }
    if entries.is_empty() {
        return EMPTY.with(Rc::clone);
    }
    let key = (entries.as_ptr() as usize, entries.len());
    let first = entries.first().unwrap().config_atom();
    let last = entries.last().unwrap().config_atom();
    CACHE.with(|c| {
        if let Some(cached) = c.borrow().get(&key)
            && cached.first_atom == first
            && cached.last_atom == last
        {
            return Rc::clone(&cached.index);
        }
        let index = Rc::new(CpBucketIndex::build(entries));
        c.borrow_mut().insert(
            key,
            CachedBucketIndex {
                first_atom: first.to_string(),
                last_atom: last.to_string(),
                index: Rc::clone(&index),
            },
        );
        index
    })
}

/// Every entry in `entries` matching this candidate, in original file order.
/// Callers sorting by [`atom_specificity`] get the old flat scan's result
/// (the sort is stable and file order is preserved here).
pub(crate) fn config_entries_matching<'a, T: ConfigAtomEntry>(
    entries: &'a [T],
    candidate_str: &str,
    category: &str,
    package: &str,
) -> Vec<&'a T> {
    config_entries_matching_with_build_id(entries, candidate_str, None, category, package)
}

/// [`config_entries_matching`] for a candidate carrying a binary build-id;
/// see [`matches_config_entry_with_build_id`].
pub(crate) fn config_entries_matching_with_build_id<'a, T: ConfigAtomEntry>(
    entries: &'a [T],
    candidate_str: &str,
    build_id: Option<&str>,
    category: &str,
    package: &str,
) -> Vec<&'a T> {
    let idx = cp_bucket_index(entries);
    idx.candidate_positions(category, package)
        .into_iter()
        .filter(|&i| {
            matches_config_entry_with_build_id(
                entries[i as usize].config_atom(),
                candidate_str,
                build_id,
                category,
                package,
            )
        })
        .map(|i| &entries[i as usize])
        .collect()
}

/// Whether any entry in `entries` matches a candidate carrying a binary
/// build-id (`None` for an ebuild-like candidate); see
/// [`matches_config_entry_with_build_id`].
pub(crate) fn any_config_entry_matches_with_build_id<T: ConfigAtomEntry>(
    entries: &[T],
    candidate_str: &str,
    build_id: Option<&str>,
    category: &str,
    package: &str,
) -> bool {
    let idx = cp_bucket_index(entries);
    idx.candidate_positions(category, package)
        .into_iter()
        .any(|i| {
            matches_config_entry_with_build_id(
                entries[i as usize].config_atom(),
                candidate_str,
                build_id,
                category,
                package,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    // The default `parse_atom` keeps rejecting extended atoms, so the index
    // routes them to `other` (re-checked one by one via
    // `matches_config_entry`); plain atoms bucket by cp.
    #[test]
    fn wildcard_entries_land_in_other_not_in_by_cp() {
        let entries = vec![
            "dev-libs/foo".to_string(),
            "dev-libs/fo*".to_string(),
            "*/foo".to_string(),
            "=*/foo-*1*".to_string(),
            "*/*:0".to_string(),
        ];
        let idx = CpBucketIndex::build(&entries);
        assert_eq!(idx.by_cp.get("dev-libs/foo").unwrap(), &vec![0]);
        assert_eq!(idx.other, vec![1, 2, 3, 4]);
        // ... and they still match through `matches_config_entry`.
        assert!(matches_config_entry(
            "dev-libs/fo*",
            "dev-libs/foo-1.0",
            "dev-libs",
            "foo"
        ));
        assert!(!matches_config_entry(
            "dev-libs/fo*",
            "dev-libs/bar-1.0",
            "dev-libs",
            "bar"
        ));
        assert!(matches_config_entry(
            "=*/foo-*1*",
            "dev-libs/foo-1.0",
            "dev-libs",
            "foo"
        ));
    }

    // Portage `WildcardPackageSet._implicitWildcarding`
    // (`lib/portage/_sets/base.py`): a category-less CLI option atom
    // matches any category; a categorised entry never gains the `*/`
    // fallback (the plain `Atom` parses first).
    #[test]
    fn cli_option_atoms_match_short_names_across_categories() {
        assert!(matches_cli_option_atom(
            "t341-dep",
            "dev-libs/t341-dep-1:0/1::t341"
        ));
        assert!(matches_cli_option_atom(
            "t341-dep:0",
            "app-misc/t341-dep-2:0/1::t341"
        ));
        assert!(!matches_cli_option_atom(
            "t341-dep:0",
            "dev-libs/t341-dep-1:1/2::t341"
        ));
        assert!(!matches_cli_option_atom(
            "t341-dep",
            "dev-libs/other-1:0/1::t341"
        ));
        // A categorised entry that fails to parse stays a miss: no `*/`
        // retry (`dev-libs/t341-dep-1` is a bare version, invalid).
        assert!(!matches_cli_option_atom(
            "dev-libs/t341-dep-1",
            "dev-libs/t341-dep-1:0/1::t341"
        ));
        // Qualified and wildcard entries behave like `match_from_list`.
        assert!(matches_cli_option_atom(
            "dev-libs/t341-dep",
            "dev-libs/t341-dep-1:0/1::t341"
        ));
        assert!(matches_cli_option_atom(
            "dev-libs/t341-*",
            "dev-libs/t341-dep-1:0/1::t341"
        ));
    }
}
