//! Per-package index over `package.*` config atoms.

use crate::*;

/// Whether `entry` (a `package.mask`/`.unmask`/`.accept_keywords` line)
/// matches this specific candidate. Two-tier: try the real, already-
/// verified `portage_dep::match_from_list` first (covers the vast
/// majority of real entries: versioned, slotted, bare atoms), and only
/// fall back to portage-dep's separate, bounded wildcard-atom matcher
/// (`*/*`, `category/*`, `*/package`) if that fails to parse `entry` at
/// all.
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

/// One `package.*` config list's atoms bucketed by `cat/pkg` -- portuale's
/// stand-in for real portage's `ExtendedAtomDict`
/// (`lib/portage/dep/__init__.py`), which every `UseManager` /
/// `MaskManager` / `KeywordsManager` / `LicenseManager` builds once at
/// config load. During a deep `emerge -pu` these lists are scanned per
/// package per graph-walk visit -- tens of thousands of times -- but only
/// the handful of entries whose atom names *this* `cp` (or a `*/*` /
/// `cat/*` / `*/pkg` wildcard) can ever match. A flat `Vec` scan with a
/// per-entry match was ~half of the post-`parse_atom`-cache run; this
/// narrows it to a `HashMap` lookup plus the wildcard tail.
#[derive(Default)]
pub(crate) struct CpBucketIndex {
    /// `"cat/pkg"` -> positions (in original file/stacking order) of every
    /// plain-atom entry for that exact `cp`.
    pub(crate) by_cp: HashMap<String, Vec<u32>>,
    /// Positions of every entry whose atom is a bounded wildcard (`*/*`,
    /// `cat/*`, `*/pkg`) or didn't parse at all -- always re-checked, kept
    /// in file order. Small on a real tree (a few `*/*` lines).
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

/// An entry in a `package.*` config list, exposing just its atom string --
/// `(atom, tokens)` for the value-carrying lists (`package.use`,
/// `package.accept_keywords`, ...) and a bare `atom` for
/// `package.mask` / `package.unmask`.
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

/// One memoised [`CpBucketIndex`]: the built index plus the first/last
/// atom strings of the list it was built from, kept as a fingerprint (see
/// [`cp_bucket_index`]).
pub(crate) struct CachedBucketIndex {
    pub(crate) first_atom: String,
    pub(crate) last_atom: String,
    pub(crate) index: Rc<CpBucketIndex>,
}

/// The [`CpBucketIndex`] for one config list, built once and memoised for
/// the life of the process. Keyed by the list's backing-storage identity
/// (`as_ptr`, `len`) with the first and last atom strings kept as a
/// fingerprint so a freed-then-reused allocation can't hand back a stale
/// index (in practice every bucketed list is a clone of stable
/// profile/user data, so a `(ptr, len)` collision already implies
/// identical content -- the fingerprint just makes that guaranteed).
/// Thread-local so the lookup stays lock-free, like the `parse_atom`
/// cache in `portage-dep`.
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

/// Every entry in `entries` that matches this candidate, in original file
/// order -- the bucketed replacement for
/// `entries.iter().filter(|e| matches_config_entry(...))`. Callers that
/// then sort by [`atom_specificity`] get an identical result to the old
/// flat scan (the sort is stable and file order is preserved here).
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

/// Whether any entry in `entries` matches -- the bucketed replacement for
/// `entries.iter().any(|e| matches_config_entry(...))`.
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
