//! Installed packages' metadata as the resolver sees it.

use crate::*;

/// Real `Package._raw_metadata` vs `Package._metadata` for an installed
/// package. `Raw` is the vdb record verbatim. `Effective` is what real
/// `FakeVartree._apply_dynamic_deps` (`_emerge/FakeVartree.py:146-191`)
/// leaves behind: the live ebuild metadata for the five `*DEPEND` keys,
/// with the vdb's own built slot-operator atoms appended back on
/// (backlog #25 S1b; the Phase 5 S1 removal's "deliberately not ported"
/// EAPI gate is what makes the append safe -- see `installed_dep_string`).
/// With `--dynamic-deps=n` (`dynamic_deps == false`) the two collapse into
/// one -- real never installs the wrapper then (`FakeVartree.py:85-91`).
///
/// The live-ebuild-gone arm is real's `_DynamicDepsNotApplicable` fallback:
/// `Raw` plus the global package-move updates (`perform_global_updates`,
/// `FakeVartree.py:184-191`). The same fallback runs when either EAPI is
/// unsupported (`FakeVartree.py:158-162`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InstalledMetaLayer {
    Raw,
    Effective,
}

/// The live ebuild metadata for one installed `cat/pkg-version` --
/// real `FakeVartree._aux_get_wrapper` (`FakeVartree.py:136`), which
/// passes `myrepo=pkg.repo` (the vdb-recorded `repository`) straight to
/// `portdb.aux_get`, so the dynamic-deps view of an installed package
/// always comes from the repo it was built from, never from a
/// repo-priority search. `None` when the version is gone from the
/// recorded repo (real's `_DynamicDepsNotApplicable` fallback: the
/// caller reads the raw vdb record instead).
///
/// Two deliberate narrowings, both documented at the call sites' shared
/// shape: when the vdb carries no `repository` file (minimal fixture
/// vdbs predate it -- e.g. `samepkg-1.0` has only `CATEGORY`/`SLOT`) or
/// the recorded repo is no longer configured, this falls back to the
/// pre-#86 `list_candidates` priority search instead of failing --
/// stranding every installed package's metadata on a removed repo (or
/// regressing every repo-less fixture vdb to the Raw view) is the worse
/// failure. Every `installed_dep_string` call site reads through this
/// one helper, so the four paths can never disagree on which repo is
/// authoritative (backlog #86).
pub(crate) fn live_metadata_for_installed(
    repos: &[RepoConfig],
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Option<std::sync::Arc<HashMap<String, String>>> {
    let pf = format!("{package}-{version}");
    let recorded = read_vdb_string(root, category, package, version, "repository");
    if !recorded.is_empty()
        && let Some(repo) = repos.iter().find(|r| r.name == recorded)
        && let Ok(meta) = repo_aux_metadata(&repo.location, category, &pf)
    {
        return Some(meta);
    }
    list_candidates(repos, category, package)
        .ok()
        .and_then(|cs| {
            cs.iter()
                .filter(|c| c.version == version)
                .max_by_key(|c| c.repo_priority)
                .and_then(|c| repo_aux_metadata(&c.repo_location, category, &pf).ok())
        })
}

/// Real `find_built_slot_operator_atoms(pkg)` (`portage/dep/_slot_operator.py:24-38`):
/// every atom in one raw (vdb) `*DEPEND` string whose
/// `Atom.slot_operator_built` is true -- `:=` **with a sub-slot**
/// (`foo/bar:0/1=`; a plain `foo/bar:2=` is *not* built,
/// `portage/dep/__init__.py:2156-2162`) -- flattened against the
/// package's recorded (`vdb/USE`) flags, in `use_reduce` order. The
/// append at `FakeVartree.py:180` is what makes the default
/// `--dynamic-deps` mode see vdb-only built bindings at all; without it
/// a consumer whose ebuild dropped the `:=` silently stops pulling the
/// built binding. A `use_reduce` failure reads as no built atoms, like
/// real's `except InvalidDependString` arm (`FakeVartree.py:172-173`).
pub(crate) fn built_slot_operator_atoms(
    raw_depstr: &str,
    use_flags: &HashSet<String>,
) -> Vec<String> {
    let tokens: Vec<String> = raw_depstr.split_whitespace().map(String::from).collect();
    let Ok(flat) = portage_use_reduce::use_reduce_flat(
        &tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
    ) else {
        return Vec::new();
    };
    flat.into_iter()
        .filter(|tok| {
            portage_dep::parse_atom(tok).is_some_and(|a| {
                a.slot_operator == Some(portage_dep::SlotOperator::Equals) && a.sub_slot.is_some()
            })
        })
        .collect()
}

/// One installed package's `*DEPEND` string for `key`, at `layer` --
/// real `Package._raw_metadata[key]` vs `Package._metadata[key]`.
///
/// `live` is the repo's current md5-cache metadata for this exact
/// `cat/pkg-version` (`None` when the version is gone from every repo;
/// `enqueue_dependencies` already returns early when its own cache read
/// fails, but the merge-order closure calls this for installed packages
/// that may have no ebuild left at all).
///
/// `memo` is keyed `(category, package, version, key)` and caches the
/// **Effective** result only, per pass. Real applies the overlay once
/// per installed instance (`FakeVartree.dynamic_deps_applied`) and is
/// explicit that a second application is a bug; a per-pass cache is the
/// portuale counterpart (the walk re-runs per backtracking pass, and
/// real's `frozen_config` would keep the first overlay across them).
/// The key stays valid with the overlay inputs: `ignore_built_slot_operator_deps`
/// is a run-constant CLI flag, and both EAPIs are stable per
/// `(category, package, version)` within a run.
///
/// One shared view for the four installed-metadata readers
/// (`reverse_dependency_constraints`, `collect_unwalked_installed_blockers`,
/// the `--deep` walk, `enqueue_dependencies`): no second copy (A1–A4).
/// Every arm below is real `FakeVartree._apply_dynamic_deps`
/// (`lib/_emerge/FakeVartree.py:146-191`), backlog #25 S1b:
/// - no live metadata, or either EAPI unsupported (`:148`, `:158-162`):
///   real's `_DynamicDepsNotApplicable` fallback (raw record + global
///   package moves). A missing vdb `EAPI` file reads as `"0"` exactly
///   like real `vartree.py:1054-1055`, so EAPI-less fixture vdbs (107 of
///   114 installed rows) take the `slot_operator`-absent arm below and
///   read live, byte-identical to before S1b.
/// - `ignore_built_slot_operator_deps` (real `--ignore-built-slot-operator-deps`,
///   `:166-168`), or an installed EAPI without `:=` support
///   (`_get_eapi_attrs(pkg.eapi).slot_operator`, `:168-169`): the live
///   ebuild string stands alone.
/// - `IDEPEND` from a live EAPI without `idepend` support (backlog #194;
///   real `bin/ebuild.sh` unsets it, `portage/eapi.py:135`): `""`.
/// - otherwise the vdb's own built `:=` atoms for this key are appended
///   to the live string (`:175-180`); when the live EAPI itself lacks
///   `:=` support the whole overlay is void (`:176-178`, raw fallback).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn installed_dep_string(
    root: &Path,
    dynamic_deps: bool,
    ignore_built_slot_operator_deps: bool,
    category: &str,
    package: &str,
    version: &str,
    live: Option<&HashMap<String, String>>,
    key: &str,
    layer: InstalledMetaLayer,
    memo: &mut HashMap<(String, String, String, String), String>,
) -> String {
    let raw = read_vdb_string(root, category, package, version, key);
    if layer == InstalledMetaLayer::Raw || !dynamic_deps {
        return raw;
    }
    let memo_key = (
        category.to_string(),
        package.to_string(),
        version.to_string(),
        key.to_string(),
    );
    if let Some(cached) = memo.get(&memo_key) {
        return cached.clone();
    }
    let result = match live {
        // Real `_DynamicDepsNotApplicable`: no live ebuild metadata, so
        // the raw record stands, with global package moves applied.
        None => apply_updates_to_dep_string(&raw).unwrap_or(raw),
        Some(live) => {
            let live_value = live
                .get(key)
                .map(String::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            // Real `FakeVartree.py:158-162` (`portage.eapi_is_supported`
            // on both EAPIs) with real `vartree.py:1054-1055`'s
            // missing-EAPI-reads-as-`"0"` normalization.
            let live_eapi = live
                .get("EAPI")
                .map(String::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let inst_eapi_raw = read_vdb_string(root, category, package, version, "EAPI");
            let inst_eapi = inst_eapi_raw.trim();
            let inst_eapi = if inst_eapi.is_empty() { "0" } else { inst_eapi };
            if !(md5_dict::eapi_is_supported(&live_eapi) && md5_dict::eapi_is_supported(inst_eapi))
            {
                apply_updates_to_dep_string(&raw).unwrap_or_else(|| raw.clone())
            } else if key == "IDEPEND" && !md5_dict::eapi_has_idepend(&live_eapi) {
                // Backlog #194: real `bin/ebuild.sh`'s own `depend`
                // phase unsets `IDEPEND` when the EAPI lacks it
                // (`portage/eapi.py:135`: `idepend = eapi >=
                // Eapi("8")`), so installed-metadata readers
                // (`FakeVartree`, serving `portdb.aux_get`) see `""`.
                // The vdb's own record needs no gate (an EAPI-7 build
                // never wrote an `IDEPEND` file); only the live-ebuild
                // overlay does.
                String::new()
            } else if ignore_built_slot_operator_deps
                || !md5_dict::eapi_has_slot_operator(inst_eapi)
            {
                live_value
            } else {
                let use_flags = read_vdb_flag_set(root, category, package, version, "USE");
                let built = built_slot_operator_atoms(&raw, &use_flags);
                if built.is_empty() {
                    live_value
                } else if !md5_dict::eapi_has_slot_operator(&live_eapi) {
                    apply_updates_to_dep_string(&raw).unwrap_or_else(|| raw.clone())
                } else if live_value.is_empty() {
                    built.join(" ")
                } else {
                    format!("{live_value} {}", built.join(" "))
                }
            }
        }
    };
    memo.insert(memo_key, result.clone());
    result
}
