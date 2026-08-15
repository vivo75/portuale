//! Per-package index over `package.*` config atoms.

use crate::*;

/// Whether `entry` (a `package.mask`/`.unmask`/`.accept_keywords` line)
/// matches this candidate: `match_from_list` first, the bounded wildcard-atom
/// matcher (`*/*`, `category/*`, `*/package`) when it fails to parse.
pub(crate) fn matches_config_entry(
    entry: &str,
    candidate_str: &str,
    category: &str,
    package: &str,
) -> bool {
    if let Some(matches) = portage_dep::match_from_list(entry, &[candidate_str]) {
        return !matches.is_empty();
    }
    match portage_dep::parse_wildcard_atom(entry) {
        Some(w) => portage_dep::wildcard_atom_matches(&w, category, package),
        None => false,
    }
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
    let idx = cp_bucket_index(entries);
    idx.candidate_positions(category, package)
        .into_iter()
        .filter(|&i| {
            matches_config_entry(
                entries[i as usize].config_atom(),
                candidate_str,
                category,
                package,
            )
        })
        .map(|i| &entries[i as usize])
        .collect()
}

/// Whether any entry in `entries` matches this candidate.
pub(crate) fn any_config_entry_matches<T: ConfigAtomEntry>(
    entries: &[T],
    candidate_str: &str,
    category: &str,
    package: &str,
) -> bool {
    let idx = cp_bucket_index(entries);
    idx.candidate_positions(category, package)
        .into_iter()
        .any(|i| {
            matches_config_entry(
                entries[i as usize].config_atom(),
                candidate_str,
                category,
                package,
            )
        })
}
