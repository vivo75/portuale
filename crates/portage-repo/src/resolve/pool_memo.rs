//! Binary-candidate pool memo for the passes of one resolve.
//!
//! `collect_binary_candidates` is pure in its arguments, the (resolve-fixed)
//! config, repos and binary indexes, and the VDB. Every pass re-asks the same
//! questions, so the answers are kept per `ResolveCtx`.

use crate::resolve::pretend::CollectBinaryCandidatesArgs;
use crate::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// One answered question: the by-value inputs and the binaries it added.
///
/// The starting ebuild pool is compared by `Arc` identity: the only producer
/// hands over `list_candidates`' process-memoised allocation (or `None` for
/// the by-construction empty pool), so the same question on a later pass
/// carries the same pointer. The bucket key already pins `(cp, atom_str)`,
/// and the only producer parses with `parse_atom`, so no parsed-atom or
/// atom-string comparison is needed.
struct PoolRow {
    flags: [bool; 8],
    root: PathBuf,
    extra_constraints: Vec<String>,
    usepkg_exclude: Vec<String>,
    usepkg_include: Vec<String>,
    ebuilds_shared: Option<Arc<Vec<Candidate>>>,
    added: Vec<Candidate>,
}

/// The memo, owned by the `ResolveCtx`.
#[derive(Default)]
pub(crate) struct PoolMemo {
    rows: RefCell<HashMap<(String, String, String), Vec<PoolRow>>>,
}

impl PoolMemo {
    const CAP_PER_KEY: usize = 8;
}

/// Identity of the ctx-owned objects the memo is valid for.
struct Scope {
    memo: Rc<PoolMemo>,
    config: *const portage_profile::Config,
    repos: (*const RepoConfig, usize),
    local: *const BinaryIndex,
}

thread_local! {
    static SCOPE: RefCell<Option<Scope>> = const { RefCell::new(None) };
}

/// Guard of [`enter`]; restores the previous scope on drop.
pub(crate) struct PoolScope(Option<Scope>);

/// Serve `collect_binary_candidates` from `memo` while the guard lives, for
/// calls made with exactly these (ctx-owned, hence alive) objects.
pub(crate) fn enter(
    memo: &Rc<PoolMemo>,
    config: &portage_profile::Config,
    repos: &[RepoConfig],
    local: &Arc<BinaryIndex>,
) -> PoolScope {
    let scope = Scope {
        memo: Rc::clone(memo),
        config,
        repos: (repos.as_ptr(), repos.len()),
        local: Arc::as_ptr(local),
    };
    PoolScope(SCOPE.with(|s| s.replace(Some(scope))))
}

impl Drop for PoolScope {
    fn drop(&mut self) {
        SCOPE.with(|s| *s.borrow_mut() = self.0.take());
    }
}

/// The scope's memo when `args` use the scope's own objects.
fn memo_for(args: &CollectBinaryCandidatesArgs<'_>) -> Option<Rc<PoolMemo>> {
    SCOPE.with(|s| {
        s.borrow().as_ref().and_then(|sc| {
            (std::ptr::eq(args.config, sc.config)
                && (args.repos.as_ptr(), args.repos.len()) == sc.repos
                && Arc::as_ptr(args.local_binpkg) == sc.local)
                .then(|| Rc::clone(&sc.memo))
        })
    })
}

/// [`collect_binary_candidates`](crate::resolve::pretend::collect_binary_candidates),
/// answered from the memo when this exact question was asked before.
/// `ebuilds_shared` is the starting pool's shared allocation (`None` for the
/// by-construction empty pool); identity implies identical input.
pub(crate) fn collect_memoized(
    args: CollectBinaryCandidatesArgs<'_>,
    ebuilds_shared: Option<&Arc<Vec<Candidate>>>,
) {
    let Some(memo) = memo_for(&args) else {
        return crate::resolve::pretend::collect_binary_candidates(args);
    };
    let flags = [
        args.usepkg,
        args.usepkgonly,
        args.getbinpkg,
        args.binpkg_respect_use,
        args.binpkg_respect_user_patches,
        args.newuse,
        args.changed_use,
        args.with_bdeps,
    ];
    let key = (
        args.atom.category.clone(),
        args.atom.package.clone(),
        args.atom_str.to_string(),
    );
    let hit = memo.rows.borrow().get(&key).and_then(|rows| {
        rows.iter()
            .find(|r| {
                r.flags == flags
                    && r.root == args.root
                    && r.extra_constraints == args.extra_constraints
                    && r.usepkg_exclude == args.usepkg_exclude
                    && r.usepkg_include == args.usepkg_include
                    && match (&r.ebuilds_shared, ebuilds_shared) {
                        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                        (None, None) => true,
                        _ => false,
                    }
            })
            .map(|r| r.added.clone())
    });
    if let Some(added) = hit {
        args.candidates.extend(added);
        return;
    }
    let start = args.candidates.len();
    let mut work = args.candidates.clone();
    crate::resolve::pretend::collect_binary_candidates(CollectBinaryCandidatesArgs {
        candidates: &mut work,
        usepkg: args.usepkg,
        usepkgonly: args.usepkgonly,
        local_binpkg: args.local_binpkg,
        atom: args.atom,
        getbinpkg: args.getbinpkg,
        config: args.config,
        root: args.root,
        binpkg_respect_use: args.binpkg_respect_use,
        binpkg_respect_user_patches: args.binpkg_respect_user_patches,
        newuse: args.newuse,
        changed_use: args.changed_use,
        repos: args.repos,
        with_bdeps: args.with_bdeps,
        atom_str: args.atom_str,
        extra_constraints: args.extra_constraints,
        usepkg_exclude: args.usepkg_exclude,
        usepkg_include: args.usepkg_include,
    });
    let added = work[start..].to_vec();
    args.candidates.extend(added.iter().cloned());
    let mut rows = memo.rows.borrow_mut();
    let slot = rows.entry(key).or_default();
    if slot.len() >= PoolMemo::CAP_PER_KEY {
        slot.remove(0);
    }
    slot.push(PoolRow {
        flags,
        root: args.root.to_path_buf(),
        extra_constraints: args.extra_constraints.to_vec(),
        usepkg_exclude: args.usepkg_exclude.to_vec(),
        usepkg_include: args.usepkg_include.to_vec(),
        ebuilds_shared: ebuilds_shared.cloned(),
        added,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args<'a>(
        config: &'a portage_profile::Config,
        local: &'a Arc<BinaryIndex>,
        atom: &'a portage_dep::Atom,
        out: &'a mut Vec<Candidate>,
        extra: &'a [String],
    ) -> CollectBinaryCandidatesArgs<'a> {
        CollectBinaryCandidatesArgs {
            usepkg: true,
            usepkgonly: false,
            local_binpkg: local,
            atom,
            getbinpkg: false,
            config,
            root: Path::new("/"),
            binpkg_respect_use: true,
            binpkg_respect_user_patches: true,
            newuse: false,
            changed_use: false,
            candidates: out,
            repos: &[],
            with_bdeps: false,
            atom_str: "dev-libs/a",
            extra_constraints: extra,
            usepkg_exclude: &[],
            usepkg_include: &[],
        }
    }

    #[test]
    fn asked_twice_is_one_row_and_foreign_objects_bypass_the_scope() {
        let memo = Rc::new(PoolMemo::default());
        let config = portage_profile::Config::default();
        let other = portage_profile::Config::default();
        let local = Arc::new(BinaryIndex::default());
        let atom = portage_dep::parse_atom("dev-libs/a").unwrap();
        let rows = || memo.rows.borrow().values().map(Vec::len).sum::<usize>();
        let mut out = Vec::new();

        // No scope: nothing recorded.
        collect_memoized(args(&config, &local, &atom, &mut out, &[]), None);
        assert_eq!(rows(), 0);
        {
            let _scope = enter(&memo, &config, &[], &local);
            collect_memoized(args(&config, &local, &atom, &mut out, &[]), None);
            collect_memoized(args(&config, &local, &atom, &mut out, &[]), None);
            assert_eq!(rows(), 1, "same question, one row");
            // A different input is a different row.
            let extra = ["<dev-libs/a-2".to_string()];
            collect_memoized(args(&config, &local, &atom, &mut out, &extra), None);
            assert_eq!(rows(), 2);
            // A config that is not the scope's own never touches the memo.
            collect_memoized(args(&other, &local, &atom, &mut out, &[]), None);
            assert_eq!(rows(), 2);
        }
        collect_memoized(args(&config, &local, &atom, &mut out, &[]), None);
        assert_eq!(rows(), 2, "the scope ended with its guard");
        assert!(out.is_empty());
    }
}
