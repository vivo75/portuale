/// Result/display-assembly unit tests: each test observes the
/// assembled structures directly, never a contract pin's text.
use crate::*;
use portage_util::TempDir;

fn new_163(version: &str) -> PretendOutcome {
    PretendOutcome::New {
        version: version.to_string(),
    }
}

fn installed_163(version: &str) -> PretendOutcome {
    PretendOutcome::AlreadyInstalled {
        version: version.to_string(),
    }
}

/// Fixed "amd64"-only, no-overrides config, mirroring `tests_162`'
/// `cfg_162` (a `KEYWORDS="amd64"` scratch ebuild is always
/// visible).
fn cfg_163() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}
// ---- shared resolve_pretend harness ----

/// Option bundle for `resolve_163`, mirroring `resolve_pretend`'s
/// trailing flags; defaults are the plain top-level selective resolve.
#[derive(Default)]
struct RpOpts163 {
    newuse: bool,
    changed_use: bool,
    update: bool,
    excluded: Vec<String>,
    changed_deps: bool,
    with_bdeps: bool,
    changed_slot: bool,
    selective: bool,
    is_top_level: bool,
    usepkg: bool,
    usepkgonly: bool,
    binpkg_respect_use: bool,
    usepkg_exclude: Vec<String>,
    usepkg_include: Vec<String>,
    rebuilt_binaries: bool,
    rebuilt_binaries_timestamp: Option<u64>,
    newrepo: bool,
    empty: bool,
    getbinpkg: bool,
    autounmask_keywords: bool,
    autounmask_use: bool,
    autounmask_license: bool,
    autounmask_masks: bool,
    extra_constraints: Vec<String>,
}

impl RpOpts163 {
    fn dep() -> Self {
        RpOpts163 {
            with_bdeps: true,
            ..Default::default()
        }
    }
}

/// Direct `resolve_pretend` call over a scratch repo + scratch vdb;
/// `binpkgs` stages `scanned_binpkgs` records like `ResolveCtx::new`.
fn resolve_163(
    repos: &[RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    atom: &str,
    o: &RpOpts163,
) -> PretendOutcome {
    resolve_pretend(
        &SelectSource {
            repos,
            root,
            config,
            local_binpkg: &build_local_binpkg_index(config),
        },
        atom,
        &SelectOptions {
            newuse: o.newuse,
            changed_use: o.changed_use,
            update: o.update,
            excluded: &o.excluded,
            changed_deps: o.changed_deps,
            with_bdeps: o.with_bdeps,
            changed_slot: o.changed_slot,
            selective: o.selective,
            is_top_level: o.is_top_level,
            usepkg: o.usepkg,
            usepkgonly: o.usepkgonly,
            binpkg_respect_use: o.binpkg_respect_use,
            usepkg_exclude: &o.usepkg_exclude,
            usepkg_include: &o.usepkg_include,
            rebuilt_binaries: o.rebuilt_binaries,
            rebuilt_binaries_timestamp: o.rebuilt_binaries_timestamp,
            newrepo: o.newrepo,
            empty: o.empty,
            getbinpkg: o.getbinpkg,
            autounmask_keywords: o.autounmask_keywords,
            autounmask_use: o.autounmask_use,
            autounmask_license: o.autounmask_license,
            autounmask_masks: o.autounmask_masks,
            extra_constraints: &o.extra_constraints,
        },
    )
    .unwrap_or_else(|e| panic!("resolve_163({atom}) failed: {e}"))
}
