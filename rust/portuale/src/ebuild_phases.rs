// Real ebuild phase execution (task #54, PORTING/PROMPT-next.md's own
// "Real ebuild phase execution" section) -- the first slice: proving real
// phase functions run and real files land under a real `${D}`, without
// touching the vdb/CONTENTS/merge machinery at all (task #55, a
// separately-scoped, much bigger piece: `dblink.merge()`/`treewalk()`/
// `mergeme()` in `lib/portage/dbapi/vartree.py`, ~6500 lines).
//
// Bash-execution backend: an embedded `brush_core::Shell` (see
// `portuale/Cargo.toml`'s own doc comment for the pinned fork/commit and
// why), driving the REAL, unmodified `bin/ebuild.sh` and the phase
// functions it sources (`bin/phase-functions.sh`/`bin/phase-helpers.sh`/
// `bin/isolated-functions.sh`/`bin/bashrc-functions.sh`/
// `bin/save-ebuild-env.sh`) -- none of that bash is reimplemented in Rust;
// this module only computes the environment `doebuild_environment()`
// would (category/version splitting, the real directory layout) and
// drives the same per-command phase sequencing `doebuild()` itself does
// (see `phase_prerequisites`'s own doc comment), leaving every actual
// phase's own execution (including real EAPI-default-phase dispatch --
// `default_src_install` etc. are themselves real bash functions in
// `phase-functions.sh`, ported here for free by sourcing that file, not
// reimplemented) entirely to that real bash.
//
// Deliberately real, but real bin/ebuild.sh has machinery this slice
// doesn't reach yet: `bin/ebuild.sh` is sourced directly (not spawned as
// its own process the way real `doebuild()`'s `spawnebuild()` does),
// deliberately without ever setting `EBUILD_SH_ARGS` -- its own trailing
// `if [[ -n ${EBUILD_SH_ARGS} ]]` block ends with a bare `exit $?`, which
// would kill the embedding Rust process rather than just return control.
// `__check_bash_version` (called unconditionally at ebuild.sh's own
// top level) checks `BASH_VERSINFO` against the EAPI's own minimum real
// bash version -- brush reports one high enough to satisfy every EAPI
// this pilot's own `portage_dep`/`portage_profile` crates already
// recognize, confirmed empirically, not by reading brush's own
// version-reporting code. Real `bin/ebuild.sh`'s own top-level code
// ALSO already sources the ebuild file itself unconditionally
// (`bin/ebuild.sh:681`'s own `source "${EBUILD}" || die`, not gated on
// `EBUILD_SH_ARGS` at all) -- confirmed empirically after an earlier
// version of this module sourced the ebuild file a *second* time
// itself, which failed ("cannot mutate readonly variable") against
// variables ebuild.sh's own tail had already made `readonly` from the
// first, automatic pass. `run_one_phase` sources `bin/ebuild.sh` alone;
// it never sources the ebuild file directly.
//
// ONE FRESH SHELL PER PHASE, not one shared across a whole invocation:
// real `bin/ebuild.sh`'s own tail makes `EBUILD_PHASE` (among other
// variables) `readonly`, so a *second* phase in the same shell can't
// `export EBUILD_PHASE=<next>` at all -- confirmed empirically the same
// way. A fresh `brush_core::Shell` per phase mirrors what real
// `doebuild()` itself does (a fresh `bin/ebuild.sh` *process* per phase,
// via `spawnebuild()`) far more literally than sharing one shell ever
// would have; real `PORTAGE_BUILDDIR`-relative resume markers
// (`.pretended`/`.setuped`/`.unpacked`/etc., written by `__dyn_*`
// themselves) are what make re-"running" an already-done prerequisite
// phase from a fresh shell cheap, exactly like real portage's own
// separate `spawnebuild()` calls rely on. See `run_one_phase`'s own doc
// comment.
//
// MUST use a multi-threaded tokio runtime (`run_commands`'s own doc
// comment): a single-threaded one deadlocks partway through a real
// multi-phase run -- confirmed empirically, and consistent with
// brush-core's own `Cargo.toml` requiring tokio's `rt-multi-thread`
// feature under unix, not just `rt`.
//
// KNOWN, DOCUMENTED GAPS (v1 scope, matching this whole pilot's own
// "narrow v1, document the cut" pattern):
//   - This module itself only runs the `actionmap_deps`-chained phases
//     for real: `pretend`, `setup`, `unpack`, `prepare`, `configure`,
//     `compile`, `test`, `install` (see `phase_prerequisites`'s own doc
//     comment). `merge`/`qmerge`/`unmerge`/`package` are real too, but
//     live in their own modules (`ebuild_merge`/`ebuild_unmerge`/
//     `ebuild_package`, each routed to directly by `ebuild.rs`, not
//     through this module's own `run_commands`). `config`/`info`/
//     `prerm`/`postrm` *are* real too, through this module's own
//     `run_single_phase` (see `is_real_standalone_phase_command`'s own
//     doc comment) -- routed to directly by `ebuild.rs`, the same way
//     `merge`/`qmerge`/`unmerge`/`package` are. Every other real
//     `ebuild` command (`preinst`/`postinst`/`nofetch`/`depend`/`fetch`/
//     `fetchall`/`digest`/`manifest`/`rpm`/`instprep`/`clean`/
//     `cleanrm`) still falls through to `ebuild.rs`'s own pre-existing
//     dry-run stub message unchanged (`preinst`/`postinst` *are* run
//     for real, but only internally, as part of `merge` -- see
//     `is_real_standalone_phase_command`'s own doc comment for why they,
//     unlike `prerm`/`postrm`, stay internal-only).
//   - No sandboxing at all (`SANDBOX_DISABLED=1` is set unconditionally
//     below) -- real portage's own `libsandbox`-based filesystem-access
//     confinement is a real, separate feature this slice doesn't attempt.
//   - `PORTAGE_PYM_PATH` (real portage's own Python-package import path)
//     is left unset -- `create_directories` pre-creates
//     `${PORTAGE_BUILDDIR}/empty` specifically so that real
//     `bin/ebuild.sh`'s own "safe cwd" logic (EAPI 8's own comment:
//     "requires us to use an empty directory here") takes that branch
//     instead of falling back to `cd "${PORTAGE_PYM_PATH}" || die`,
//     which would otherwise `die` immediately on every single phase
//     (confirmed empirically). `__save_ebuild_env`'s own
//     environment.bz2-writing path (used by `pkg_preinst`/`pkg_postinst`
//     hooks this slice doesn't reach) still separately fails non-fatally
//     when it tries to `cd "${PORTAGE_PYM_PATH}"` with no `|| die` guard
//     -- observed as harmless `TARGET_DIR`/`chgrp` warning noise on
//     every phase, not something that stops a phase from completing.
//   - `__source_all_bashrcs` (real per-profile/package bashrc hook
//     support, `/etc/portage/bashrc` and friends) is left unimplemented
//     -- also observed as a non-fatal "command not found" warning, not a
//     phase failure. This pilot has no profile/make.conf-driven bashrc
//     concept anywhere yet.
//   - Starting from the third phase in any given `ebuild_phases::
//     run_commands` call, sourcing `bin/ebuild.sh` prints six additional
//     "cannot mutate readonly variable" warnings (real `bin/ebuild.sh`'s
//     own `readonly SANDBOX_{ALLOW,ACTIVE,DENY,DEBUG,ON,PREDICT,READ,
//     WRITE}` declaration) -- observed, not yet root-caused (each phase
//     gets a genuinely fresh `Shell`, so this isn't the same
//     shared-shell issue `run_one_phase`'s own doc comment describes;
//     plausibly brush's own environment-variable inheritance across
//     `Shell` instances within one OS process, still unconfirmed).
//     Cosmetic: every phase still completes and returns the correct
//     exit status regardless.
//   - `EAPI` is read via the real PMS 7.3.1 rule directly from the
//     ebuild's own text (see `parse_eapi`) rather than through this
//     pilot's own md5-cache-reading machinery (`portage-repo`'s own
//     `read_md5_cache`) -- real `ebuild <file> <command>` operates on an
//     arbitrary standalone ebuild file, not necessarily one that's part
//     of a configured, md5-cache-indexed repo, so this mirrors real
//     `_parse_eapi_ebuild_head` instead.
//   - `PORTAGE_TMPDIR` defaults to `/var/tmp/portage` (real portage's own
//     `make.globals` default) but is overridable via the `PORTAGE_TMPDIR`
//     environment variable -- this pilot has no make.conf-reading path
//     into `ebuild.rs` at all yet, so an env var is the only override
//     mechanism, the same "env var, not full config resolution"
//     simplification `ROOT`/`PORTAGE_CONFIGROOT` already established for
//     `emerge`.
//   - `FILESDIR` (real `${PORTAGE_BUILDDIR}/files`, itself populated by
//     `prepare_build_dirs()` copying the repo's own `<category>/
//     <package>/files/` into it) is created here empty and never
//     populated -- this slice's own fixture ebuild deliberately avoids
//     `FILESDIR` entirely (writes its own scratch file under `${T}`
//     instead) rather than needing that copy step ported too.

use crate::fetch::{self, FetchOptions};
use brush_builtins::ShellBuilderExt as _;
use regex::Regex;
use std::path::{Path, PathBuf};

/// Real PMS 7.3.1: the ebuild's own EAPI is the value of an `EAPI=...`
/// assignment (optionally single- or double-quoted, an optional trailing
/// comment) on the first non-blank, non-comment line -- and *only* that
/// line; an `EAPI=` assignment anywhere else in the file is not the
/// ebuild's own EAPI at all (PMS's own rationale: the EAPI must be
/// knowable without evaluating any bash). No match at all (including an
/// ebuild whose first real line isn't an EAPI assignment) means EAPI "0",
/// matching real `doebuild.py`'s own "eapi = None" fallback. Mirrors real
/// `lib/portage/__init__.py`'s own `_parse_eapi_ebuild_head`/
/// `_pms_eapi_re` exactly.
fn parse_eapi(ebuild_text: &str) -> String {
    // Real PMS's own `\1` backreference (matching whichever quote char, if
    // any, opened the value) isn't expressible in Rust's `regex` crate
    // (deliberately no backreference support, for guaranteed linear-time
    // matching) -- expanded into three explicit alternatives instead
    // (double-quoted / single-quoted / bare), semantically identical.
    let eapi_re = Regex::new(
        r#"^[ \t]*EAPI=(?:"([A-Za-z0-9+_.-]*)"|'([A-Za-z0-9+_.-]*)'|([A-Za-z0-9+_.-]*))[ \t]*([ \t]#.*)?$"#,
    )
    .expect("static regex is valid");
    let comment_or_blank = Regex::new(r"^\s*(#.*)?$").expect("static regex is valid");

    for line in ebuild_text.lines() {
        if comment_or_blank.is_match(line) {
            continue;
        }
        return match eapi_re.captures(line) {
            Some(caps) => caps
                .get(1)
                .or_else(|| caps.get(2))
                .or_else(|| caps.get(3))
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string(),
            None => "0".to_string(),
        };
    }
    "0".to_string()
}

/// Real `doebuild.py`'s own `_pkgsplit`-derived `P`/`PN`/`PV`/`PR`/`PVR`.
/// Real `_pkgsplit` derives `PN` from the version-shaped *suffix* of a
/// bare `PF` string with no other information -- this pilot's own
/// `portage-versions` crate doesn't port that algorithm (it only has
/// `ververify`/`vercmp`, not a name/version splitter), so this reuses the
/// same shortcut `portage-repo`'s own `strip_version_prefix` already
/// relies on instead: `PN` is simply the ebuild's own *parent directory*
/// name (real convention -- `<category>/<package>/<package>-<version>.
/// ebuild` -- that `doebuild_environment`'s own `os.path.basename(pkg_dir)
/// in (mysplit[0], mypv)` assertion actually checks holds, rather than
/// derives from scratch). `PR` is the trailing `-r<digits>` suffix of
/// what's left after stripping `PN-` (default `"r0"`, real portage's own
/// "no explicit revision" default) with `PV` being everything before it.
pub(crate) struct PackageSplit {
    pub(crate) p: String,
    pub(crate) pn: String,
    /// Real `PV`: the version *without* any `-r<digits>` revision suffix.
    pub(crate) pv: String,
    /// Real `PR`: `"r0"` when no explicit `-r<digits>` suffix was present
    /// (real portage's own "no explicit revision" default), otherwise the
    /// suffix itself.
    pub(crate) pr: String,
    /// Real `PVR`: `PV` alone when `PR` is `"r0"`, otherwise `PV-PR` --
    /// real portage's own "omit r0 from display" convention.
    pub(crate) pvr: String,
    pub(crate) pf: String,
}

fn split_package(ebuild_path: &Path, package_dir_name: &str) -> Result<PackageSplit, String> {
    let pf = ebuild_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: not a valid file path", ebuild_path.display()))?
        .to_string();
    let pvr = pf
        .strip_prefix(package_dir_name)
        .and_then(|rest| rest.strip_prefix('-'))
        .ok_or_else(|| {
            format!(
                "{}: filename doesn't start with the parent directory's own name ({package_dir_name:?})",
                ebuild_path.display()
            )
        })?
        .to_string();
    let revision_re = Regex::new(r"^(.*)-(r\d+)$").expect("static regex is valid");
    let (pv, pr) = match revision_re.captures(&pvr) {
        Some(caps) => (caps[1].to_string(), caps[2].to_string()),
        None => (pvr.clone(), "r0".to_string()),
    };
    let pn = package_dir_name.to_string();
    let p = format!("{pn}-{pv}");
    Ok(PackageSplit {
        p,
        pn,
        pv,
        pr,
        pvr,
        pf,
    })
}

/// Real `doebuild.py:874-884`'s own `actionmap_deps`: the prerequisite
/// chain for `mydo`, run in order before `mydo` itself -- ported here as
/// the Rust-side driver loop (see this module's own doc comment for why
/// everything else stays real bash). Only the phase-only subset is
/// covered (see this module's own "KNOWN, DOCUMENTED GAPS"); an
/// unrecognized `mydo` returns just itself, letting the caller decide
/// whether that's valid for its own purposes.
fn phase_prerequisites(mydo: &str) -> Vec<&'static str> {
    const CHAIN: &[&str] = &[
        "pretend",
        "setup",
        "unpack",
        "prepare",
        "configure",
        "compile",
        "test",
        "install",
    ];
    match CHAIN.iter().position(|&p| p == mydo) {
        Some(idx) => CHAIN[..=idx].to_vec(),
        None => Vec::new(),
    }
}

/// Whether `command` is one this module can actually execute for real
/// (the `actionmap_deps`-chained phase subset) -- `ebuild.rs` checks this
/// before routing to `run_commands`, falling back to its own pre-existing
/// dry-run stub for everything else.
pub fn is_real_phase_command(command: &str) -> bool {
    phase_prerequisites(command).last() == Some(&command)
}

/// Whether `command` is a real, standalone single-phase `ebuild`
/// command with no `actionmap_deps` prerequisite chain at all -- real
/// `doebuild()`'s own `mydo in ("config", "help", "info", "postinst",
/// "preinst", "pretend", "postrm", "prerm")` early-return branch
/// (`lib/portage/package/ebuild/doebuild.py:1326-1351`, "running them
/// out of the sandbox -- and stop now"), narrowed to the four of those a
/// real admin/user actually invokes directly by name:
/// `config`/`info`/`prerm`/`postrm` -- `preinst`/`postinst` are real
/// too, but only ever reached internally, as part of `merge` (see
/// `ebuild_merge::run_merge`'s own doc comment: real `dblink.
/// treewalk()` invokes them directly around the actual file-copy step,
/// a real ordering constraint no standalone top-level invocation could
/// reproduce -- `preinst` must run *before* anything is merged,
/// `postinst` only *after*), so they stay internal-only; `pretend` is
/// already part of the `actionmap_deps` chain above; `help` this
/// pilot's own CLI already handles separately (`wants_help`).
/// `prerm`/`postrm` have no equivalent ordering constraint tying them to
/// `unmerge`'s own file-removal step the way `preinst`/`postinst` do to
/// `merge`'s -- real portage itself allows invoking them completely
/// standalone (e.g. to test a `pkg_prerm`/`pkg_postrm` function without
/// actually removing the package), so `unmerge`'s own internal use
/// (`ebuild_unmerge::run_unmerge`'s own doc comment) and this new
/// standalone path are simply two independent, real ways to reach the
/// same real phase function. `run_single_phase` (already used
/// internally for exactly this reason) is a direct fit for all four:
/// real `bin/phase-functions.sh`'s own `__ebuild_main` already accepts
/// them as literal phase arguments (`run_single_phase`'s own doc
/// comment), so no new phase-execution machinery is needed at all --
/// this is purely a CLI-routing addition.
pub fn is_real_standalone_phase_command(command: &str) -> bool {
    matches!(command, "config" | "info" | "prerm" | "postrm")
}

/// The real directory layout `doebuild_environment()` computes (all
/// paths, `PORTAGE_BUILDDIR`-relative, matching real
/// `lib/portage/package/ebuild/doebuild.py:499-524` exactly): `D`/`ED`
/// get a trailing separator, matching real portage's own convention
/// (every real helper script's own path-joining assumes it).
pub(crate) struct Environment {
    pub(crate) ebuild_abs: PathBuf,
    pub(crate) pkg_dir: PathBuf,
    pub(crate) category: String,
    pub(crate) split: PackageSplit,
    eapi: String,
    portage_builddir: PathBuf,
}

pub(crate) fn compute_environment(
    ebuild_path: &Path,
    portage_tmpdir: &Path,
) -> Result<Environment, String> {
    let ebuild_abs = ebuild_path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", ebuild_path.display()))?;
    let pkg_dir = ebuild_abs
        .parent()
        .ok_or_else(|| format!("{}: has no parent directory", ebuild_abs.display()))?
        .to_path_buf();
    // Real doebuild_environment(): `cat = os.path.basename(os.path.
    // dirname(pkg_dir))` -- the category is the ebuild's own
    // grandparent directory name (<category>/<package>/<pf>.ebuild).
    let category = pkg_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            format!(
                "{}: cannot determine CATEGORY from path",
                ebuild_abs.display()
            )
        })?
        .to_string();

    let package_dir_name = pkg_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: cannot determine package name", pkg_dir.display()))?;

    let ebuild_text = std::fs::read_to_string(&ebuild_abs)
        .map_err(|e| format!("{}: {e}", ebuild_abs.display()))?;
    let eapi = parse_eapi(&ebuild_text);
    let split = split_package(&ebuild_abs, package_dir_name)?;

    let portage_builddir = portage_tmpdir
        .join("portage")
        .join(&category)
        .join(&split.pf);

    Ok(Environment {
        ebuild_abs,
        pkg_dir,
        category,
        split,
        eapi,
        portage_builddir,
    })
}

impl Environment {
    pub(crate) fn d(&self) -> PathBuf {
        self.portage_builddir.join("image")
    }
    fn workdir(&self) -> PathBuf {
        self.portage_builddir.join("work")
    }
    fn t(&self) -> PathBuf {
        self.portage_builddir.join("temp")
    }
    fn s(&self) -> PathBuf {
        self.workdir().join(&self.split.p)
    }
    fn home(&self) -> PathBuf {
        self.portage_builddir.join("homedir")
    }
    fn filesdir(&self) -> PathBuf {
        self.portage_builddir.join("files")
    }
    /// Real `${PORTAGE_BUILDDIR}/build-info`: created as a side effect of
    /// every real `unpack|prepare|configure|compile|test|clean|install`
    /// phase already run by the time `ebuild_package::run_package`'s own
    /// `install` chain completes (`bin/phase-functions.sh`'s own
    /// unconditional `mkdir build-info` in that case branch) -- so by
    /// the time packaging needs it, it already exists and already has a
    /// real copy of the ebuild file in it (`build-info/${PF}.ebuild`).
    pub(crate) fn build_info(&self) -> PathBuf {
        self.portage_builddir.join("build-info")
    }
    /// Real `${PORTAGE_BUILDDIR}/.installed`: real, unmodified
    /// `bin/phase-functions.sh`'s own `__dyn_install` already creates
    /// this unconditionally on a successful `src_install`
    /// (`phase-functions.sh:653`, no `FEATURES` gate at all) -- this
    /// pilot writes nothing new for it, real phase execution already
    /// leaves it behind as a side effect (confirmed empirically: a real
    /// `ebuild <file> install` run via this pilot's own binary leaves
    /// `.installed` in place). `ebuild_merge::run_qmerge` is the one
    /// caller: real `doebuild()`'s own `mydo == "qmerge"` branch checks
    /// for exactly this marker before skipping the install phase.
    pub(crate) fn installed_marker(&self) -> PathBuf {
        self.portage_builddir.join(".installed")
    }
}

/// Real `bin/*.sh` needs every directory it writes into (via helpers, or
/// bare bash redirection) to already exist -- `__dyn_unpack` creates
/// `WORKDIR` itself, but `T`/`D`/`HOME` are relied on existing already
/// (real `doebuild()` creates them via `prepare_build_dirs()` before
/// spawning the phase at all).
///
/// `S` is created here too, empty, as a deliberate v1 simplification:
/// real `S` only ever exists because `src_unpack`'s own `unpack ${A}`
/// call creates it as a side effect of extracting a real, fetched
/// `SRC_URI` archive -- this pilot has no fetch/unpack machinery at all
/// (no network access attempted, no archive-format support), so an
/// ebuild whose own `A` is empty (real `default_src_unpack`'s own
/// `[[ -n ${A} ]] && unpack ${A}` never runs `unpack` at all in that
/// case) would otherwise reach `src_prepare`/`src_configure`/
/// `src_compile`/`src_install` with a `${S}` that flat-out doesn't
/// exist, since nothing else creates it either. Real ebuilds with a
/// nonempty `SRC_URI` are simply out of scope for this slice (would need
/// real fetch+unpack support, its own separately-scoped follow-up); this
/// pre-creation only matters for exactly the empty-`SRC_URI` case this
/// slice's own fixture (and any similarly source-less ebuild) exercises.
fn create_directories(env: &Environment) -> Result<(), String> {
    for dir in [
        env.t(),
        env.d(),
        env.home(),
        env.filesdir(),
        env.s(),
        // Real `bin/ebuild.sh`'s own top-level code (run unconditionally
        // as soon as it's sourced, EAPI 8's own comment: "requires us to
        // use an empty directory here"): `cd`s into `${PORTAGE_BUILDDIR}/
        // empty` if it exists, falling back to `${PORTAGE_PYM_PATH}`
        // (unset in this pilot -- no Python-package-path concept at all)
        // otherwise, `die`-ing if neither works. Always pre-created here
        // so that fallback path, which this pilot can't satisfy, is
        // never reached at all.
        env.portage_builddir.join("empty"),
    ] {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

pub(crate) fn repo_root() -> PathBuf {
    // portuale/src/ebuild_phases.rs -> portuale -> rust -> PORTING -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../")
        .canonicalize()
        .expect("repo root resolves (portuale is always built from within the real checkout)")
}

/// Real portage's own mechanism for locating a repo from one of its own
/// ebuild files: walks up from `pkg_dir` looking for a `profiles/
/// repo_name` file, returning the *ancestor directory itself* (the real
/// repo root, suitable for `portage_repo::read_md5_cache`) -- `None` for
/// a standalone ebuild file outside any repo checkout. Shared by
/// `fetch_sources` below and `ebuild_package.rs`'s own `Packages`-index
/// metadata lookup (moved here rather than duplicated, since both need
/// exactly the same walk).
pub(crate) fn repo_root_for(pkg_dir: &Path) -> Option<PathBuf> {
    for ancestor in pkg_dir.ancestors() {
        if ancestor.join("profiles").join("repo_name").is_file() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// Real `doebuild()`'s own `SRC_URI`-vs-`DISTDIR` fetch check (see
/// `crate::fetch`'s own module doc comment for the real mechanics),
/// run once per `run_commands_async` call rather than per-phase.
/// `SRC_URI` itself is read from the ebuild's own repo's real
/// `metadata/md5-cache` entry (the same source `ebuild_package.rs`
/// already trusts for `Packages`-index metadata) -- absent entirely
/// (no fetch attempted, `A`/`AA` both empty) for a standalone ebuild
/// file outside any repo checkout, the same tolerance `repo_root_for`
/// already established. Returns `(A, AA)`: `A` is the real, actually-
/// fetched filename list (this pilot's own always-empty USE set, see
/// `crate::fetch::fetch_src_uri`'s own doc comment); `AA` is every
/// filename `SRC_URI` could ever reference regardless of USE (real
/// PMS's own definition), computed but never itself fetched.
fn fetch_sources(env: &Environment, distdir: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let src_uri = portage_repo::read_md5_cache(&repo_root, &env.category, &env.split.pf)
        .ok()
        .and_then(|metadata| metadata.get("SRC_URI").cloned())
        .unwrap_or_default();
    if src_uri.trim().is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let aa = portage_fetch::flatten_src_uri(&src_uri, |_, _| true)
        .map_err(|e| format!("{}: {e}", env.pkg_dir.display()))?
        .into_iter()
        .map(|entry| entry.filename)
        .collect();
    let a = fetch::fetch_src_uri(
        &env.pkg_dir,
        &src_uri,
        &FetchOptions {
            distdir: distdir.to_path_buf(),
            gentoo_mirrors: fetch::gentoo_mirrors_from_env(),
            // Real PORTAGE_CONFIGROOT (real default: "/" when unset) --
            // consulted only for real `custommirrors`, see
            // `FetchOptions::config_root`'s own doc comment.
            config_root: portage_repo::config_root_from_env(),
            // Real `"distlocks" in self.settings.features` -- same
            // env-var-not-full-config-resolution shortcut
            // `collision_protect`/`protect_owned`/`unmerge_orphans`
            // already use, defaulting to real `true` (see
            // `FetchOptions::distlocks`'s own doc comment).
            distlocks: std::env::var("FEATURES")
                .map(|features| features.split_whitespace().any(|tok| tok == "distlocks"))
                .unwrap_or(FetchOptions::default().distlocks),
        },
    )?;
    Ok((a, aa))
}

/// Which real shell executes a phase, and every real `bin/*.sh` this
/// pilot sources unmodified along with it: `Brush` (the default -- an
/// embedded `brush_core::Shell`, see this module's own doc comment for
/// why) or `Bash`, a genuine `bash <bin_dir>/ebuild.sh <phase>`
/// subprocess -- matching real portage's own `_doebuild_spawn()`
/// invocation shape almost exactly (`lib/portage/package/ebuild/
/// doebuild.py`'s own `cmd = "{ebuild.sh} {phase}"`, spawned via
/// `portage.process.spawn()`; real `bin/ebuild.sh:153`'s own
/// `EBUILD_SH_ARGS="$*"` picks up `<phase>` from the subprocess's own
/// positional args, which its own tail, `bin/ebuild.sh:830-843`, then
/// really uses to call `__ebuild_main ${EBUILD_SH_ARGS}` and `exit` --
/// the brush backend deliberately never sets `EBUILD_SH_ARGS` at all,
/// since a bare `exit` inside an *embedded* shell would kill the whole
/// hosting Rust process rather than just return control, see this
/// module's own doc comment; a real subprocess has no such problem, so
/// `Bash` uses that real mechanism directly instead of brush's own
/// "source, then separately `invoke_function`" two-step). Selectable
/// via `ebuild --shell bash|brush` (see `ebuild.rs`'s own CLI wiring)
/// -- a pilot-only flag, not a real `bin/ebuild` option, so it's
/// deliberately NOT in `ebuild_options::OPTIONS` (that table is
/// specifically a transcription of real bin/ebuild's own argparse
/// setup).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellBackend {
    #[default]
    Brush,
    Bash,
}

/// The real environment-variable block every real phase and every real
/// `bin/misc-functions.sh` `__dyn_*` command alike needs, as raw
/// `(name, value)` pairs -- shared by both shell backends (`Brush`
/// formats these into `export NAME=value` bash source text, see
/// `phase_setup_script` below; `Bash` passes them directly as real
/// subprocess environment variables, `std::process::Command::envs`,
/// with no shell-quoting step -- and so no `$`/backtick-expansion risk
/// -- at all) and by `run_one_phase`/`run_misc_functions` so the two
/// don't duplicate this. `extra_env` is appended verbatim, for anything
/// specific to one call site (e.g. `ebuild_package`'s own `PKGDIR`/
/// `PORTAGE_BINPKG_TMPFILE`).
/// Real `bin/ebuild.sh`'s own `eval "PORTAGE_ECLASS_LOCATIONS=(${{PORTAGE_ECLASS_LOCATIONS}})"`
/// (line ~611, run once, unconditionally, right after being sourced)
/// expects this env var's own raw string value to already be a
/// sequence of shell-single-quoted path tokens -- real `doebuild.py`'s
/// own `repo.eclass_db.eclass_locations_string` (`shlex.join(...)`)
/// builds it the same way. `inherit()` itself (also real, unmodified
/// bash) then walks that array looking for `<location>/eclass/
/// <name>.eclass` for every eclass named on an ebuild's own top-level
/// `inherit ...` line -- previously this pilot never populated this
/// var at all, so `inherit()` always `die`d immediately for ANY
/// eclass, confirmed live against a real system: `sys-fs/fuse`,
/// `app-editors/nano`, and `app-arch/xz-utils` all failed here before
/// this fix, each on a different real eclass.
///
/// Real masters-chain resolution (`config.py:1256-1266`, `RepoConfigLoader.
/// __init__`): `eclass_locations = [master.location for master in
/// repo.masters] + ([repo.location] if repo.location not already in
/// there)`, then `eclass_db.eclass_locations_string` exports it
/// `reversed()` (`eclass_cache.py:177-179`) -- the ebuild's own
/// containing repo searched *first*, its masters after, in real
/// declared order. `repo.masters` itself (`RepoConfig::masters`, the
/// same real, already-resolved chain `ebuild_merge::
/// blocked_installed_packages` and `pretend.rs` already consult for
/// profile/USE config stacking) defaults to the main repo alone when no
/// explicit `masters =` key is present, empty for the main repo itself.
/// `repos.conf`/`config_root` resolution failure of any kind (missing
/// `repos.conf`, the containing repo not listed in it, etc.) degrades
/// to the previous v1 behavior -- the ebuild's own containing repo
/// alone, no masters chain -- the same graceful-degrade precedent
/// `blocked_installed_packages`'s own doc comment already established
/// for this exact `find_repos(config_root).ok()?` pattern: never a
/// false negative in the direction that could break an eclass lookup
/// that used to work. `None` (a standalone ebuild file outside any repo
/// checkout) exports an empty value either way -- `inherit()` still
/// `die`s for any eclass in that case, the same honest "nothing to
/// look in" real behavior a truly master-less, repo-less ebuild file
/// would hit too.
fn eclass_locations_value(pkg_dir: &Path, config_root: &Path) -> String {
    let Some(repo_root) = repo_root_for(pkg_dir) else {
        return String::new();
    };

    let masters: Vec<PathBuf> = (|| -> Option<Vec<PathBuf>> {
        let repos = portage_repo::find_repos(config_root).ok()?;
        let repo = repos.iter().find(|r| r.location == repo_root)?;
        Some(repo.masters.clone())
    })()
    .unwrap_or_default();

    let mut locations = masters;
    if !locations.contains(&repo_root) {
        locations.push(repo_root);
    }
    locations.reverse();

    locations
        .iter()
        .map(|p| format!("'{}'", p.display().to_string().replace('\'', r"'\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(clippy::too_many_arguments)]
fn phase_env_vars(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    debug: bool,
    bin_dir: &Path,
    helpers_dir: &Path,
    config_root: &Path,
    extra_env: &[(String, String)],
) -> Vec<(String, String)> {
    // Real EPREFIX support is never enabled here (`EPREFIX=""`
    // unconditionally below), so real `ED="${D}"` (no prefix-relative
    // adjustment) collapses to exactly `D`'s own value -- no separate
    // computation needed.
    let d = format!("{}/", env.d().display());
    let path = format!(
        "{}:{}",
        helpers_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut vars = vec![
        ("EAPI".to_string(), env.eapi.clone()),
        ("PN".to_string(), env.split.pn.clone()),
        ("PV".to_string(), env.split.pv.clone()),
        ("PR".to_string(), env.split.pr.clone()),
        ("PVR".to_string(), env.split.pvr.clone()),
        ("P".to_string(), env.split.p.clone()),
        ("PF".to_string(), env.split.pf.clone()),
        ("CATEGORY".to_string(), env.category.clone()),
        ("EBUILD".to_string(), env.ebuild_abs.display().to_string()),
        ("O".to_string(), env.pkg_dir.display().to_string()),
        ("ROOT".to_string(), root.display().to_string()),
        ("EROOT".to_string(), root.display().to_string()),
        (
            "PORTAGE_BUILDDIR".to_string(),
            env.portage_builddir.display().to_string(),
        ),
        ("WORKDIR".to_string(), env.workdir().display().to_string()),
        ("S".to_string(), env.s().display().to_string()),
        ("D".to_string(), d.clone()),
        ("ED".to_string(), d),
        ("T".to_string(), env.t().display().to_string()),
        ("HOME".to_string(), env.home().display().to_string()),
        ("FILESDIR".to_string(), env.filesdir().display().to_string()),
        (
            "PORTAGE_BIN_PATH".to_string(),
            bin_dir.display().to_string(),
        ),
        (
            "PORTAGE_ECLASS_LOCATIONS".to_string(),
            eclass_locations_value(&env.pkg_dir, config_root),
        ),
        ("PORTAGE_PYTHON".to_string(), "/usr/bin/python".to_string()),
        ("PATH".to_string(), path),
        ("SANDBOX_DISABLED".to_string(), "1".to_string()),
        ("FEATURES".to_string(), String::new()),
        ("USE".to_string(), String::new()),
        ("EPREFIX".to_string(), String::new()),
        ("EMERGE_FROM".to_string(), "ebuild".to_string()),
        ("PORTAGE_QUIET".to_string(), "1".to_string()),
        (
            "PORTAGE_DEBUG".to_string(),
            if debug { "1" } else { "0" }.to_string(),
        ),
        ("EBUILD_PHASE".to_string(), ebuild_phase_value.to_string()),
    ];
    vars.extend(extra_env.iter().cloned());
    vars
}

/// `Brush`-backend-only: `phase_env_vars` formatted as real `export
/// NAME=value` bash source text (Rust's own `{:?}` Debug-format
/// double-quoted escaping -- not a full shell-quoting implementation,
/// so a value containing `$`/backtick isn't protected against
/// expansion, but every value here is this pilot's own computed path/
/// metadata text, never arbitrary ebuild-controlled content). `Bash`
/// backend needs no such text at all -- `phase_env_vars`'s own pairs
/// are passed directly as real subprocess environment variables
/// instead, see `run_one_phase_bash`/`run_misc_functions_bash`.
#[allow(clippy::too_many_arguments)]
fn phase_setup_script(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    debug: bool,
    bin_dir: &Path,
    helpers_dir: &Path,
    config_root: &Path,
    extra_env: &[(String, String)],
) -> String {
    let mut script = String::new();
    for (name, value) in phase_env_vars(
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    ) {
        script.push_str(&format!("export {name}={value:?}\n"));
    }
    script
}

/// Builds one fresh embedded brush shell for a single phase: real
/// `doebuild()` spawns a brand new `bin/ebuild.sh` *process* per phase
/// (`spawnebuild()`), which matters for more than just isolation --
/// `bin/ebuild.sh`'s own tail makes `EBUILD_PHASE` (among other
/// variables) `readonly` (`declare -r`), so a real fresh process is the
/// only way a *second* phase can `export EBUILD_PHASE=<next>` at all.
/// This pilot mirrors that exactly with a fresh `Shell` per phase rather
/// than trying to reuse one across phases (an earlier version of this
/// function did share one shell across a whole command's own prerequisite
/// chain -- confirmed empirically to fail with "cannot mutate readonly
/// variable" on the second phase, exactly the real readonly-variable
/// mechanism working as designed). Real `PORTAGE_BUILDDIR`-relative
/// resume markers (`.pretended`/`.setuped`/`.unpacked`/etc., written by
/// `__dyn_*` themselves) still make a prerequisite phase that's already
/// run cheap to "re-run" from a fresh shell, exactly the way real
/// `doebuild()` itself relies on across its own separate `spawnebuild()`
/// calls -- this isn't a new mechanism invented for this pilot.
#[allow(clippy::too_many_arguments)]
async fn run_one_phase(
    env: &Environment,
    root: &Path,
    phase: &str,
    debug: bool,
    extra_env: &[(String, String)],
    config_root: &Path,
    shell: ShellBackend,
) -> Result<i32, String> {
    let bin_dir = repo_root().join("bin");
    let helpers_dir = bin_dir.join("ebuild-helpers");

    match shell {
        ShellBackend::Brush => {
            run_one_phase_brush(
                env,
                root,
                phase,
                debug,
                extra_env,
                &bin_dir,
                &helpers_dir,
                config_root,
            )
            .await
        }
        ShellBackend::Bash => run_one_phase_bash(
            env,
            root,
            phase,
            debug,
            extra_env,
            &bin_dir,
            &helpers_dir,
            config_root,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_one_phase_brush(
    env: &Environment,
    root: &Path,
    phase: &str,
    debug: bool,
    extra_env: &[(String, String)],
    bin_dir: &Path,
    helpers_dir: &Path,
    config_root: &Path,
) -> Result<i32, String> {
    let mut shell = brush_core::Shell::builder()
        .default_builtins(brush_builtins::BuiltinSet::BashMode)
        .build()
        .await
        .map_err(|e| format!("brush shell failed to start: {e}"))?;
    let params = shell.default_exec_params();

    let setup = phase_setup_script(
        env,
        root,
        phase,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    );
    shell
        .run_string(&setup, &brush_core::SourceInfo::default(), &params)
        .await
        .map_err(|e| format!("environment setup failed: {e}"))?;

    // Real bin/ebuild.sh's own top-level code (unconditional, not gated
    // on EBUILD_SH_ARGS at all -- confirmed empirically, not just by
    // reading it: `bin/ebuild.sh:681`'s own `source "${EBUILD}" || die`
    // sits in ebuild.sh's own main body) ALREADY sources the ebuild file
    // itself as part of being sourced -- a second, separate
    // `source_script` call on the ebuild file here would be genuinely
    // redundant, not just wasteful: it re-runs the ebuild's own
    // top-level code a second time against variables ebuild.sh's own
    // tail has *already* made `readonly` from the first pass, which
    // fails outright ("cannot mutate readonly variable") -- confirmed
    // empirically by removing this line and watching that error
    // disappear.
    shell
        .source_script(
            bin_dir.join("ebuild.sh"),
            std::iter::empty::<String>(),
            &params,
        )
        .await
        .map_err(|e| format!("sourcing bin/ebuild.sh failed: {e}"))?;

    shell
        .invoke_function("__ebuild_main", [phase], params)
        .await
        .map_err(|e| format!("phase {phase} failed: {e}"))
        .map(u8::into)
}

/// `--shell bash`: spawns a genuine `bash <bin_dir>/ebuild.sh <phase>`
/// subprocess instead of the embedded `brush_core::Shell` `run_one_
/// phase_brush` above uses -- see `ShellBackend`'s own doc comment for
/// why this mirrors real portage's own `_doebuild_spawn()` invocation
/// shape (`EBUILD_SH_ARGS="$*"` picking `<phase>` up from real argv)
/// far more directly than the brush path's own two-step "source, then
/// separately `invoke_function`" dance does. Environment variables are
/// real subprocess env vars (`phase_env_vars`), not shell `export`
/// source text, so there's no shell-quoting step -- and so no `$`/
/// backtick-expansion risk -- at all, unlike `phase_setup_script`'s own
/// Rust-Debug escaping. A blocking `std::process::Command`, not a
/// `tokio::process` one: matches `fetch.rs`'s own precedent for
/// spawning a real subprocess (`wget`) from inside an `async fn`
/// without pulling in tokio's own "process" feature.
#[allow(clippy::too_many_arguments)]
fn run_one_phase_bash(
    env: &Environment,
    root: &Path,
    phase: &str,
    debug: bool,
    extra_env: &[(String, String)],
    bin_dir: &Path,
    helpers_dir: &Path,
    config_root: &Path,
) -> Result<i32, String> {
    let vars = phase_env_vars(
        env,
        root,
        phase,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    );
    let status = std::process::Command::new("bash")
        .arg(bin_dir.join("ebuild.sh"))
        .arg(phase)
        .envs(vars)
        .status()
        .map_err(|e| format!("spawning real bash for phase {phase} failed: {e}"))?;
    Ok(status.code().unwrap_or(1))
}

/// Real `bin/misc-functions.sh`'s own invocation shape -- unlike
/// `run_one_phase`'s own `bin/ebuild.sh` + `__ebuild_main <phase>`, real
/// `doebuild()` invokes commands like `"package"` as a *separate*
/// script, `bin/misc-functions.sh __dyn_<mydo>` (real
/// `lib/portage/package/ebuild/doebuild.py`'s own `misc_sh = ... +
/// " __dyn_%s"`), not through `bin/ebuild.sh`'s own phase dispatch at
/// all -- confirmed by reading it: `bin/phase-functions.sh`'s own
/// `__ebuild_main` case statement has no `"package"` branch whatsoever.
/// `misc-functions.sh` itself sources `bin/ebuild.sh` (inheriting the
/// same environment/ebuild-sourcing this pilot's own `run_one_phase`
/// already relies on), captures its own positional args into
/// `MISC_FUNCTIONS_ARGS` *before* sourcing (so `ebuild.sh`'s own arg
/// handling never sees them), then its own tail unconditionally runs
/// `for x in ${MISC_FUNCTIONS_ARGS}; do ${x}; done` -- i.e. sourcing it
/// with `dyn_command` as a positional arg is enough to invoke it
/// directly; no separate `invoke_function` call is needed the way
/// `run_one_phase`'s own explicit `__ebuild_main` call is.
#[allow(clippy::too_many_arguments)]
async fn run_misc_functions(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    dyn_command: &str,
    extra_env: &[(String, String)],
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
) -> Result<i32, String> {
    let bin_dir = repo_root().join("bin");
    let helpers_dir = bin_dir.join("ebuild-helpers");

    match shell {
        ShellBackend::Brush => {
            run_misc_functions_brush(
                env,
                root,
                ebuild_phase_value,
                dyn_command,
                extra_env,
                debug,
                &bin_dir,
                &helpers_dir,
                config_root,
            )
            .await
        }
        ShellBackend::Bash => run_misc_functions_bash(
            env,
            root,
            ebuild_phase_value,
            dyn_command,
            extra_env,
            debug,
            &bin_dir,
            &helpers_dir,
            config_root,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_misc_functions_brush(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    dyn_command: &str,
    extra_env: &[(String, String)],
    debug: bool,
    bin_dir: &Path,
    helpers_dir: &Path,
    config_root: &Path,
) -> Result<i32, String> {
    let mut shell = brush_core::Shell::builder()
        .default_builtins(brush_builtins::BuiltinSet::BashMode)
        .build()
        .await
        .map_err(|e| format!("brush shell failed to start: {e}"))?;
    let params = shell.default_exec_params();

    let setup = phase_setup_script(
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    );
    shell
        .run_string(&setup, &brush_core::SourceInfo::default(), &params)
        .await
        .map_err(|e| format!("environment setup failed: {e}"))?;

    shell
        .source_script(
            bin_dir.join("misc-functions.sh"),
            [dyn_command.to_string()].into_iter(),
            &params,
        )
        .await
        .map_err(|e| format!("running {dyn_command} failed: {e}"))
        .map(|result| i32::from(u8::from(result.exit_code)))
}

/// `--shell bash`: spawns a genuine `bash <bin_dir>/misc-functions.sh
/// <dyn_command>` subprocess -- matching real `doebuild.py`'s own
/// `misc_sh = shlex.quote(misc_sh_binary) + " __dyn_%s"` invocation
/// shape exactly. See `run_one_phase_bash`'s own doc comment for why a
/// blocking `std::process::Command` here too.
#[allow(clippy::too_many_arguments)]
fn run_misc_functions_bash(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    dyn_command: &str,
    extra_env: &[(String, String)],
    debug: bool,
    bin_dir: &Path,
    helpers_dir: &Path,
    config_root: &Path,
) -> Result<i32, String> {
    let vars = phase_env_vars(
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    );
    let status = std::process::Command::new("bash")
        .arg(bin_dir.join("misc-functions.sh"))
        .arg(dyn_command)
        .envs(vars)
        .status()
        .map_err(|e| format!("spawning real bash for {dyn_command} failed: {e}"))?;
    Ok(status.code().unwrap_or(1))
}

/// Synchronous entry point mirroring `run_single_phase`'s own shape, for
/// `ebuild_package`'s own real `__dyn_package` call.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_misc_function(
    ebuild_path: &Path,
    portage_tmpdir: &Path,
    root: &Path,
    ebuild_phase_value: &str,
    dyn_command: &str,
    extra_env: &[(String, String)],
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
) -> Result<i32, String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to start async runtime: {e}"))?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        run_misc_functions(
            &env,
            root,
            ebuild_phase_value,
            dyn_command,
            extra_env,
            debug,
            config_root,
            shell,
        )
        .await
    })
}

/// Drives `commands` against `ebuild_path` for real: computes the
/// environment once, then for each of `commands` in order, runs its own
/// `phase_prerequisites` chain, each phase in its own fresh embedded
/// brush shell (see `run_one_phase`'s own doc comment for why a fresh
/// shell per phase, not one shared across the whole invocation, is the
/// real, not simplified, model here) sourcing real `bin/ebuild.sh` (see
/// this module's own doc comment) and the ebuild file itself.
///
/// Real `doebuild()`'s own `SRC_URI`-vs-`DISTDIR` fetch check (see
/// `fetch_sources`'s own doc comment) runs exactly once here, before
/// the phase loop, whenever the combined prerequisite chain includes
/// `unpack` -- matching real portage's own ordering (real `pkg_pretend`
/// explicitly runs *before* fetching, PMS's whole point for that phase
/// being a fast sanity check that shouldn't need network access at
/// all; `setup` likewise precedes it). The resulting `A`/`AA` are
/// exported into *every* phase this call runs, not just `unpack`
/// itself, matching real portage's own environment (every phase sees
/// the same `A`/`AA`, whether or not it happens to reference them).
/// `DISTDIR` itself is always exported too (regardless of whether
/// `unpack` is in the chain at all), matching real portage's own
/// unconditional environment -- real `unpack` (a `bin/ebuild-helpers/`
/// script, not reimplemented) resolves `${A}`'s own files relative to
/// `${DISTDIR}` itself, not anything this Rust code passes it
/// directly, so omitting the export here would silently break real
/// unpacking even after a real, successful fetch (caught empirically:
/// a real fetched-and-verified distfile still made `unpack` report
/// `"either does not exist or is not a regular file"` before this was
/// added).
#[allow(clippy::too_many_arguments)]
async fn run_commands_async(
    ebuild_path: &Path,
    commands: &[&str],
    root: &Path,
    portage_tmpdir: &Path,
    distdir: &Path,
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
) -> Result<i32, String> {
    let env = compute_environment(ebuild_path, portage_tmpdir)?;
    create_directories(&env)?;

    let chain: Vec<&str> = commands
        .iter()
        .flat_map(|&c| phase_prerequisites(c))
        .collect();
    let mut extra_env = vec![("DISTDIR".to_string(), distdir.display().to_string())];
    if chain.contains(&"unpack") {
        let (a, aa) = fetch_sources(&env, distdir)?;
        extra_env.push(("A".to_string(), a.join(" ")));
        extra_env.push(("AA".to_string(), aa.join(" ")));
    }

    for &command in commands {
        for phase in phase_prerequisites(command) {
            let status =
                run_one_phase(&env, root, phase, debug, &extra_env, config_root, shell).await?;
            if status != 0 {
                return Ok(status);
            }
            // Real `_post_phase_cmds["install"]` (`EbuildPhase.py:424`/
            // `442-461`): real, unconditional `bin/misc-functions.sh
            // install_qa_check install_symlink_html_docs install_hooks`,
            // run once right after a successful real `install` phase --
            // not gated on any `FEATURES` flag, and (unlike `ebuild
            // <file> package`'s own separate `__dyn_package` misc-
            // functions call) never itself part of `phase_prerequisites`'
            // own chain, so this is the one place it can run. `EBUILD_
            // PHASE` stays `"install"` for this call, matching real
            // portage's own behavior (`_PostPhaseCommands` reuses the
            // exact same `settings` the install phase itself already
            // used, never resetting it). Real `bin/misc-functions.sh`'s
            // own `MISC_FUNCTIONS_ARGS="$@"` then unquoted `for x in
            // ${MISC_FUNCTIONS_ARGS}` re-splits on whitespace regardless
            // of how many real argv entries this arrived as, so passing
            // all three names as one space-joined string here is exactly
            // equivalent to real portage's own three separate positional
            // args -- `run_misc_functions` needs no changes at all.
            if phase == "install" {
                let qa_status = run_misc_functions(
                    &env,
                    root,
                    "install",
                    "install_qa_check install_symlink_html_docs install_hooks",
                    &extra_env,
                    debug,
                    config_root,
                    shell,
                )
                .await?;
                if qa_status != 0 {
                    return Ok(qa_status);
                }
            }
        }
    }
    Ok(0)
}

/// Synchronous entry point for `ebuild.rs` (which is not itself async --
/// `emerge`'s own dispatch never needs an async runtime at all, so this
/// pilot doesn't pay for one there; only this one code path does). Spins
/// up a tokio runtime for the duration of the call -- MUST be
/// multi-threaded (`new_multi_thread`, not `new_current_thread`):
/// confirmed empirically (a single-threaded runtime deadlocks partway
/// through a real multi-phase run -- brush-core's own `Cargo.toml`
/// requires tokio's `rt-multi-thread` feature under unix, not just
/// `rt`, which is the same thing this pilot rediscovered the hard way).
///
/// `PORTAGE_TMPDIR` (real portage's own `make.globals` default:
/// `/var/tmp/portage`) is read by the caller, not internally here --
/// deliberately, so tests can pass a distinct value directly rather than
/// mutating process-global environment state (`std::env::set_var` is
/// unsound to call from parallel test threads), the same "env var read
/// once at the CLI boundary" shape `emerge`'s own `pretend.rs` already
/// uses for `ROOT`/`PORTAGE_CONFIGROOT` via `root_from_env`/
/// `config_root_from_env`.
#[allow(clippy::too_many_arguments)]
pub fn run_commands(
    ebuild_path: &Path,
    commands: &[&str],
    root: &Path,
    portage_tmpdir: &Path,
    distdir: &Path,
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
) -> Result<i32, String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to start async runtime: {e}"))?;
    runtime.block_on(run_commands_async(
        ebuild_path,
        commands,
        root,
        portage_tmpdir,
        distdir,
        debug,
        config_root,
        shell,
    ))
}

/// Runs exactly `phase`, with no `actionmap_deps` prerequisite chain --
/// unlike `run_commands`, for phases real portage itself never reaches
/// via `doebuild()`'s own chain at all. Real `dblink.treewalk()` invokes
/// `pkg_preinst`/`pkg_postinst` directly (`EbuildPhase(phase="preinst"/
/// "postinst")`, `lib/portage/dbapi/vartree.py`), not through
/// `doebuild(mydo=...)` -- `ebuild_merge::run_merge` is this pilot's own
/// equivalent call site, wrapping its own file-merge step with real
/// `pkg_preinst`/`pkg_postinst` hook execution the same way. Real
/// `bin/phase-functions.sh`'s own `__ebuild_main` already accepts
/// `preinst`/`postinst` as literal phase arguments directly (`case
/// prerm|postrm|preinst|postinst|config|info)`), and `__ebuild_phase`
/// itself silently no-ops when the named function isn't defined
/// (`declare -F "$1" >/dev/null && __qa_call $1`) -- so this is safe to
/// call even for a fixture ebuild that defines neither `pkg_preinst` nor
/// `pkg_postinst` at all.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_single_phase(
    ebuild_path: &Path,
    phase: &str,
    root: &Path,
    portage_tmpdir: &Path,
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
) -> Result<i32, String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to start async runtime: {e}"))?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        // No `A`/`AA` here: real `pkg_preinst`/`pkg_postinst` run after
        // `install`'s own real `unpack` already completed (real
        // `dblink.treewalk()` invokes them directly, never through
        // `doebuild()`'s own fetch-then-phases sequence at all -- see
        // this function's own doc comment), so there's nothing to
        // re-fetch or re-export here.
        run_one_phase(&env, root, phase, debug, &[], config_root, shell).await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_eapi_reads_the_first_real_lines_own_assignment() {
        assert_eq!(parse_eapi("EAPI=8\nDESCRIPTION=x\n"), "8");
        assert_eq!(parse_eapi("EAPI=\"8\"\n"), "8");
        assert_eq!(parse_eapi("EAPI='8'\n"), "8");
        assert_eq!(parse_eapi("# comment\n\nEAPI=7\n"), "7");
        assert_eq!(parse_eapi("EAPI=8 # trailing comment\n"), "8");
    }

    #[test]
    fn parse_eapi_defaults_to_0_when_the_first_real_line_is_not_an_assignment() {
        assert_eq!(parse_eapi("DESCRIPTION=x\nEAPI=8\n"), "0");
        assert_eq!(parse_eapi(""), "0");
        assert_eq!(parse_eapi("# only comments\n"), "0");
    }

    #[test]
    fn phase_prerequisites_chains_up_to_and_including_the_requested_phase() {
        assert_eq!(phase_prerequisites("pretend"), vec!["pretend"]);
        assert_eq!(
            phase_prerequisites("compile"),
            vec![
                "pretend",
                "setup",
                "unpack",
                "prepare",
                "configure",
                "compile"
            ]
        );
        assert_eq!(
            phase_prerequisites("install"),
            vec![
                "pretend",
                "setup",
                "unpack",
                "prepare",
                "configure",
                "compile",
                "test",
                "install"
            ]
        );
    }

    #[test]
    fn phase_prerequisites_is_empty_for_an_unrecognized_command() {
        assert_eq!(phase_prerequisites("merge"), Vec::<&str>::new());
        assert_eq!(phase_prerequisites("qmerge"), Vec::<&str>::new());
    }

    #[test]
    fn is_real_phase_command_covers_exactly_the_actionmap_deps_chain() {
        for cmd in [
            "pretend",
            "setup",
            "unpack",
            "prepare",
            "configure",
            "compile",
            "test",
            "install",
        ] {
            assert!(
                is_real_phase_command(cmd),
                "{cmd} should be a real phase command"
            );
        }
        for cmd in [
            "merge", "qmerge", "unmerge", "package", "clean", "digest", "info",
        ] {
            assert!(
                !is_real_phase_command(cmd),
                "{cmd} should NOT be a real phase command"
            );
        }
    }

    #[test]
    fn is_real_standalone_phase_command_covers_exactly_config_info_prerm_postrm() {
        for cmd in ["config", "info", "prerm", "postrm"] {
            assert!(
                is_real_standalone_phase_command(cmd),
                "{cmd} should be a real standalone phase command"
            );
        }
        for cmd in [
            "merge", "qmerge", "unmerge", "package", "clean", "install", "preinst", "postinst",
            "help",
        ] {
            assert!(
                !is_real_standalone_phase_command(cmd),
                "{cmd} should NOT be a real standalone phase command"
            );
        }
    }

    #[test]
    fn split_package_separates_pv_from_the_revision() {
        let split =
            split_package(Path::new("/repo/dev-libs/foo/foo-1.2.3-r1.ebuild"), "foo").unwrap();
        assert_eq!(split.pn, "foo");
        assert_eq!(split.pv, "1.2.3");
        assert_eq!(split.pr, "r1");
        assert_eq!(split.pvr, "1.2.3-r1");
        assert_eq!(split.p, "foo-1.2.3");
        assert_eq!(split.pf, "foo-1.2.3-r1");
    }

    #[test]
    fn split_package_defaults_pr_to_r0_when_no_revision_is_present() {
        let split = split_package(Path::new("/repo/dev-libs/foo/foo-1.0.ebuild"), "foo").unwrap();
        assert_eq!(split.pv, "1.0");
        assert_eq!(split.pr, "r0");
        assert_eq!(split.pvr, "1.0");
        assert_eq!(split.pf, "foo-1.0");
    }

    #[test]
    fn split_package_rejects_a_filename_not_matching_the_parent_directory() {
        assert!(split_package(Path::new("/repo/dev-libs/foo/bar-1.0.ebuild"), "foo").is_err());
    }

    /// End-to-end proof: a real `install` run against the real fixture
    /// ebuild (`PORTING/fixtures/repo/dev-libs/phasepkg`, whose own
    /// `src_install` calls real `insinto`/`doins`) actually lands a real
    /// file under a real `${D}`, via the real, unmodified `bin/*.sh`
    /// this module drives -- not a mock of any kind. A fresh, uniquely
    /// named `PORTAGE_TMPDIR` per test run (passed directly to
    /// `run_commands`, never via the environment -- see its own doc
    /// comment for why) keeps this safe to run alongside other tests in
    /// the same process.
    #[test]
    fn install_lands_a_real_file_under_a_real_d() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "install_lands_a_real_file_under_a_real_d"
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);

        let status = run_commands(
            &ebuild_path,
            &["install"],
            Path::new("/"),
            &portage_tmpdir,
            &portage_tmpdir.join("distfiles"),
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Brush,
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should exit successfully");

        let installed =
            portage_tmpdir.join("portage/dev-libs/phasepkg-1.0/image/usr/share/phasepkg/hello.txt");
        let contents = std::fs::read_to_string(&installed)
            .unwrap_or_else(|e| panic!("{} should have been installed: {e}", installed.display()));
        assert_eq!(contents, "hello from phasepkg\n");

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }

    /// Real, end-to-end proof that `_post_phase_cmds["install"]`
    /// (`EbuildPhase.py:424`/`442-461`) actually runs now: real,
    /// unmodified `bin/misc-functions.sh install_qa_check`'s own real
    /// `95empty-dirs` QA check (`bin/install-qa-check.d/95empty-dirs`)
    /// strips a genuinely empty directory from the install image for
    /// any EAPI 8+ ebuild (real `___eapi_has_strict_keepdir`,
    /// unconditional, not gated on any `FEATURES` flag) -- a bare
    /// `dodir` with nothing ever installed into it must be gone from
    /// `${D}` by the time `install` finishes, while a `keepdir`'d one
    /// (the real ebuild-author idiom this QA check's own message
    /// recommends) survives untouched.
    #[test]
    fn install_runs_the_real_post_install_qa_check_and_strips_a_genuinely_empty_dir() {
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-install_runs_the_real_post_install_qa_check",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let repo_root = tmp.join("repo");
        let pkg_dir = repo_root.join("dev-libs/qacheckpkg");
        std::fs::create_dir_all(repo_root.join("profiles")).unwrap();
        std::fs::write(repo_root.join("profiles/repo_name"), "qachecktest\n").unwrap();
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("qacheckpkg-1.0.ebuild"),
            "EAPI=8\n\
             DESCRIPTION=\"fixture: real post-install QA check strips a genuinely empty dir\"\n\
             SLOT=\"0\"\n\
             KEYWORDS=\"amd64\"\n\
             src_install() {\n\
             \tdodir /usr/lib/reallyempty\n\
             \tkeepdir /usr/lib/keptempty\n\
             }\n",
        )
        .unwrap();

        let ebuild_path = pkg_dir.join("qacheckpkg-1.0.ebuild");
        let portage_tmpdir = tmp.join("portage_tmpdir");

        let status = run_commands(
            &ebuild_path,
            &["install"],
            Path::new("/"),
            &portage_tmpdir,
            &portage_tmpdir.join("distfiles"),
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Brush,
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should exit successfully");

        let image_dir = portage_tmpdir.join("portage/dev-libs/qacheckpkg-1.0/image");
        assert!(
            !image_dir.join("usr/lib/reallyempty").exists(),
            "a genuinely empty dodir'd directory must be stripped by the real post-install QA check"
        );
        assert!(
            image_dir.join("usr/lib/keptempty").is_dir(),
            "a keepdir'd directory (real ebuild-author idiom) must survive"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Real, end-to-end proof that standalone `config`/`info` (real
    /// `ebuild.rs`'s own routing to `run_single_phase`, see
    /// `is_real_standalone_phase_command`'s own doc comment) actually
    /// runs the real `pkg_config`/`pkg_info` phase functions -- not just
    /// that `run_single_phase` returns successfully. No `install` chain
    /// involved at all, matching real standalone usage (a real admin
    /// runs `ebuild <file> config` directly against an ebuild, with no
    /// merge/vdb step in the same invocation).
    #[test]
    fn run_single_phase_actually_runs_pkg_config_and_pkg_info() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/standalonephasepkg/standalonephasepkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "run_single_phase_actually_runs_pkg_config_and_pkg_info"
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);

        let config_status = run_single_phase(
            &ebuild_path,
            "config",
            Path::new("/"),
            &portage_tmpdir,
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Brush,
        )
        .expect("run_single_phase should not itself error");
        assert_eq!(config_status, 0);
        let info_status = run_single_phase(
            &ebuild_path,
            "info",
            Path::new("/"),
            &portage_tmpdir,
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Brush,
        )
        .expect("run_single_phase should not itself error");
        assert_eq!(info_status, 0);

        let t_dir = portage_tmpdir.join("portage/dev-libs/standalonephasepkg-1.0/temp");
        assert!(
            t_dir.join("pkg-config-ran").is_file(),
            "pkg_config must actually run"
        );
        assert!(
            t_dir.join("pkg-info-ran").is_file(),
            "pkg_info must actually run"
        );

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }

    /// Real, end-to-end proof that standalone `prerm`/`postrm` actually
    /// run the real `pkg_prerm`/`pkg_postrm` phase functions -- same
    /// shape as `run_single_phase_actually_runs_pkg_config_and_pkg_info`
    /// above, but for the two standalone commands that also have a real,
    /// separate internal use (`ebuild_unmerge::run_unmerge`, see
    /// `is_real_standalone_phase_command`'s own doc comment for why
    /// that internal use and this new standalone path are simply two
    /// independent ways to reach the same real phase function). No
    /// `unmerge`/vdb step involved at all here, matching real standalone
    /// usage.
    #[test]
    fn run_single_phase_actually_runs_pkg_prerm_and_pkg_postrm() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/standalonephasepkg/standalonephasepkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "run_single_phase_actually_runs_pkg_prerm_and_pkg_postrm"
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);

        let prerm_status = run_single_phase(
            &ebuild_path,
            "prerm",
            Path::new("/"),
            &portage_tmpdir,
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Brush,
        )
        .expect("run_single_phase should not itself error");
        assert_eq!(prerm_status, 0);
        let postrm_status = run_single_phase(
            &ebuild_path,
            "postrm",
            Path::new("/"),
            &portage_tmpdir,
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Brush,
        )
        .expect("run_single_phase should not itself error");
        assert_eq!(postrm_status, 0);

        let t_dir = portage_tmpdir.join("portage/dev-libs/standalonephasepkg-1.0/temp");
        assert!(
            t_dir.join("pkg-prerm-ran").is_file(),
            "pkg_prerm must actually run"
        );
        assert!(
            t_dir.join("pkg-postrm-ran").is_file(),
            "pkg_postrm must actually run"
        );

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }

    /// `ShellBackend::Bash` counterpart of `install_lands_a_real_file_
    /// under_a_real_d` above -- same fixture, same assertions, proving
    /// the real `bash <bin_dir>/ebuild.sh <phase>` subprocess backend
    /// (`run_one_phase_bash`) produces an identical real result to the
    /// embedded-brush backend, not just that it runs without erroring.
    #[test]
    fn install_lands_a_real_file_under_a_real_d_via_real_bash() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "install_lands_a_real_file_under_a_real_d_via_real_bash"
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);

        let status = run_commands(
            &ebuild_path,
            &["install"],
            Path::new("/"),
            &portage_tmpdir,
            &portage_tmpdir.join("distfiles"),
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Bash,
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should exit successfully");

        let installed =
            portage_tmpdir.join("portage/dev-libs/phasepkg-1.0/image/usr/share/phasepkg/hello.txt");
        let contents = std::fs::read_to_string(&installed)
            .unwrap_or_else(|e| panic!("{} should have been installed: {e}", installed.display()));
        assert_eq!(contents, "hello from phasepkg\n");

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }

    /// Real, end-to-end proof of `fetch_sources`'s own `A`/`AA`
    /// computation through the real CLI path, deterministic and
    /// offline: `dev-libs/verifiedfetchpkg`'s own real, checked-in
    /// `Manifest` entry matches a payload file this test pre-seeds into
    /// `DISTDIR` (the real, valid BLAKE2b-512/SHA-512 digests of the
    /// literal bytes `"hello from verifiedfetchpkg\n"`, confirmed via
    /// the real `b2sum`/`sha512sum` system tools) -- so the "already
    /// verified" skip-fetch path fires and no real network access is
    /// attempted at all, while still exercising the full real SRC_URI
    /// grammar: an arrow-rename (`-> verifiedfetchpkg-1.0.tar.gz`) and a
    /// `test?` USE-conditional group that must stay excluded from `A`
    /// (this pilot's own always-empty USE set) but still appear in
    /// `AA` (real PMS's own "every file regardless of USE" definition).
    #[test]
    fn install_computes_real_a_and_aa_from_a_verified_distfile_with_no_network() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/verifiedfetchpkg/verifiedfetchpkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "install_computes_real_a_and_aa_from_a_verified_distfile_with_no_network"
        ));
        let distdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-distdir-{}-{}",
            std::process::id(),
            "install_computes_real_a_and_aa_from_a_verified_distfile_with_no_network"
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);
        let _ = std::fs::remove_dir_all(&distdir);
        std::fs::create_dir_all(&distdir).unwrap();
        std::fs::write(
            distdir.join("verifiedfetchpkg-1.0.tar.gz"),
            b"hello from verifiedfetchpkg\n",
        )
        .unwrap();

        let status = run_commands(
            &ebuild_path,
            &["install"],
            Path::new("/"),
            &portage_tmpdir,
            &distdir,
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Brush,
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should exit successfully");

        let marker =
            portage_tmpdir.join("portage/dev-libs/verifiedfetchpkg-1.0/temp/fetch-vars.txt");
        let observed = std::fs::read_to_string(&marker)
            .unwrap_or_else(|e| panic!("{} should have been written: {e}", marker.display()));
        assert_eq!(
            observed,
            "A=verifiedfetchpkg-1.0.tar.gz\n\
             AA=verifiedfetchpkg-1.0.tar.gz verifiedfetchpkg-tests-1.0.tar.gz\n"
        );

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
        let _ = std::fs::remove_dir_all(&distdir);
    }

    /// Real, end-to-end proof of `eclass_locations_value`: `dev-libs/
    /// eclasspkg` really `inherit`s a real (if fixture-only) eclass,
    /// `pilotcheck.eclass`, via real, unmodified `bin/ebuild.sh`'s own
    /// `inherit()` function -- previously this pilot never populated
    /// `PORTAGE_ECLASS_LOCATIONS` at all, so this would have `die`d
    /// immediately with `"pilotcheck.eclass could not be found by
    /// inherit()"`. `src_install` calls a real function the eclass
    /// defines (`pilotcheck_hello`), proving the eclass's own content
    /// -- not just its own existence -- is really usable afterward.
    #[test]
    fn install_really_inherits_a_real_eclass_and_calls_its_own_function() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/eclasspkg/eclasspkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "install_really_inherits_a_real_eclass_and_calls_its_own_function"
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);

        let status = run_commands(
            &ebuild_path,
            &["install"],
            Path::new("/"),
            &portage_tmpdir,
            &portage_tmpdir.join("distfiles"),
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Brush,
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should exit successfully");

        let marker = portage_tmpdir.join("portage/dev-libs/eclasspkg-1.0/temp/eclass-marker.txt");
        let observed = std::fs::read_to_string(&marker)
            .unwrap_or_else(|e| panic!("{} should have been written: {e}", marker.display()));
        assert_eq!(observed, "hello from pilotcheck.eclass\n");

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }

    /// Regression test for a real upstream brush bug (fixed in the pinned
    /// fork, see README.md's own eclass section for the full writeup):
    /// `bigfixture.eclass` defines ~400 functions so that real
    /// `bin/phase-functions.sh`'s own post-phase `__save_ebuild_env |
    /// __filter_readonly_variables` pipe (both sides real shell
    /// functions) carries well over the OS pipe buffer size (~64KiB on
    /// Linux) worth of `declare -f` output. Before the fix, brush ran a
    /// function used as a non-last pipeline stage inline rather than as
    /// a background task, so the pipeline-spawning loop blocked on
    /// `__save_ebuild_env` fully returning before even spawning
    /// `__filter_readonly_variables` to drain it -- a real, reproducible
    /// deadlock, not a slow completion (confirmed against real
    /// app-arch/xz-utils and sys-fs/fuse before the fix, which both
    /// inherit the real `multilib` eclass family). Run on a background
    /// thread with a hard deadline so a regression here fails this test
    /// outright instead of hanging the whole suite.
    #[test]
    fn install_does_not_deadlock_on_an_eclass_scope_larger_than_the_pipe_buffer() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/bigeclasspkg/bigeclasspkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "install_does_not_deadlock_on_an_eclass_scope_larger_than_the_pipe_buffer"
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);

        let (tx, rx) = std::sync::mpsc::channel();
        let thread_ebuild_path = ebuild_path.clone();
        let thread_portage_tmpdir = portage_tmpdir.clone();
        std::thread::spawn(move || {
            let result = run_commands(
                &thread_ebuild_path,
                &["install"],
                Path::new("/"),
                &thread_portage_tmpdir,
                &thread_portage_tmpdir.join("distfiles"),
                false,
                Path::new("/dev/null/no-config-root"),
                ShellBackend::Brush,
            );
            let _ = tx.send(result);
        });
        let status = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("run_commands should complete well within the deadline, not deadlock")
            .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should exit successfully");

        let marker =
            portage_tmpdir.join("portage/dev-libs/bigeclasspkg-1.0/temp/bigfixture-marker.txt");
        let observed = std::fs::read_to_string(&marker)
            .unwrap_or_else(|e| panic!("{} should have been written: {e}", marker.display()));
        assert_eq!(observed, "hello from bigfixture.eclass\n");

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }

    #[test]
    fn eclass_locations_value_quotes_the_containing_repo_root() {
        // Canonicalized first, matching what `compute_environment`
        // always hands `repo_root_for` in the real path (it always
        // canonicalizes the ebuild's own path before deriving
        // `pkg_dir` from it).
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo")
            .canonicalize()
            .unwrap();
        let pkg_dir = repo_root.join("dev-libs/eclasspkg");
        let value = eclass_locations_value(&pkg_dir, Path::new("/dev/null/no-config-root"));
        // Real bin/ebuild.sh's own `eval "PORTAGE_ECLASS_LOCATIONS=(${...})"`
        // expects single-quoted tokens -- confirmed by round-tripping
        // through the exact same real, unmodified bash line here.
        assert_eq!(value, format!("'{}'", repo_root.display()));
    }

    #[test]
    fn eclass_locations_value_is_empty_outside_any_repo_checkout() {
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-eclass_locations_value_none",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("dev-libs/standalone");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        assert_eq!(
            eclass_locations_value(&pkg_dir, Path::new("/dev/null/no-config-root")),
            ""
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn eclass_locations_value_puts_the_own_repo_first_then_masters_in_declared_order() {
        // Real config.py:1256-1266 + eclass_cache.py:177-179: `eclass_
        // locations = [master.location for master in repo.masters] +
        // [repo.location]`, exported `reversed()` -- so the ebuild's own
        // containing repo is searched first, its masters after, in real
        // declared order.
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-eclass_locations_masters_order",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let main = tmp.join("main");
        let secondary = tmp.join("secondary");
        let overlay = tmp.join("overlay");
        for repo in [&main, &secondary, &overlay] {
            std::fs::create_dir_all(repo.join("profiles")).unwrap();
        }
        std::fs::write(main.join("profiles/repo_name"), "main\n").unwrap();
        std::fs::write(secondary.join("profiles/repo_name"), "secondary\n").unwrap();
        std::fs::write(overlay.join("profiles/repo_name"), "overlay\n").unwrap();
        std::fs::create_dir_all(tmp.join("etc/portage")).unwrap();
        std::fs::write(
            tmp.join("etc/portage/repos.conf"),
            format!(
                "[DEFAULT]\nmain-repo = main\n\n\
                 [main]\nlocation = {}\n\n\
                 [secondary]\nlocation = {}\n\n\
                 [overlay]\nlocation = {}\nmasters = main secondary\n",
                main.display(),
                secondary.display(),
                overlay.display(),
            ),
        )
        .unwrap();

        let pkg_dir = overlay.join("dev-libs/overlaypkg");
        let value = eclass_locations_value(&pkg_dir, &tmp);
        assert_eq!(
            value,
            format!(
                "'{}' '{}' '{}'",
                overlay.display(),
                secondary.display(),
                main.display()
            ),
            "own repo first, then masters in real declared order"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn eclass_locations_value_does_not_duplicate_the_own_repo_when_it_is_also_a_master() {
        // Real config.py:1264-1266: "Only append the current repo to
        // eclass_locations if it's not there already" -- exercised via
        // the main repo itself, whose own `masters` real-defaults to
        // empty (`config.py:1229-1260`, "the main repo can never be its
        // own master"), so this also doubles as a real-default-masters
        // proof: no explicit `masters =` key at all for `main`.
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-eclass_locations_no_dup",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let main = tmp.join("main");
        std::fs::create_dir_all(main.join("profiles")).unwrap();
        std::fs::write(main.join("profiles/repo_name"), "main\n").unwrap();
        std::fs::create_dir_all(tmp.join("etc/portage")).unwrap();
        std::fs::write(
            tmp.join("etc/portage/repos.conf"),
            format!(
                "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = {}\n",
                main.display()
            ),
        )
        .unwrap();

        let pkg_dir = main.join("dev-libs/mainpkg");
        let value = eclass_locations_value(&pkg_dir, &tmp);
        assert_eq!(value, format!("'{}'", main.display()));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Real, end-to-end proof that the masters-chain fix actually
    /// unblocks a real `inherit()` call real `PORTAGE_ECLASS_LOCATIONS`
    /// resolution alone couldn't reach before: an overlay ebuild
    /// inheriting an eclass that only exists in its own master repo,
    /// never redeclared locally -- exactly the real gap this module's
    /// own doc comment (before this slice) named as out of scope.
    #[test]
    fn install_inherits_a_real_eclass_that_only_exists_in_the_overlays_own_master_repo() {
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-eclass_masters_e2e",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let main = tmp.join("main");
        let overlay = tmp.join("overlay");
        std::fs::create_dir_all(main.join("profiles")).unwrap();
        std::fs::create_dir_all(main.join("eclass")).unwrap();
        std::fs::write(main.join("profiles/repo_name"), "main\n").unwrap();
        std::fs::write(
            main.join("eclass/mastershared.eclass"),
            "mastershared_hello() {\n\techo \"hello from mastershared.eclass\"\n}\n",
        )
        .unwrap();

        let pkg_dir = overlay.join("dev-libs/overlaypkg");
        std::fs::create_dir_all(overlay.join("profiles")).unwrap();
        std::fs::write(overlay.join("profiles/repo_name"), "overlay\n").unwrap();
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("overlaypkg-1.0.ebuild"),
            "EAPI=8\n\
             DESCRIPTION=\"fixture: real cross-repo masters-chain eclass inherit\"\n\
             SLOT=\"0\"\n\
             KEYWORDS=\"amd64\"\n\
             inherit mastershared\n\
             src_install() {\n\
             \tmastershared_hello > \"${T}/eclass-marker.txt\" || die\n\
             }\n",
        )
        .unwrap();

        std::fs::create_dir_all(tmp.join("etc/portage")).unwrap();
        std::fs::write(
            tmp.join("etc/portage/repos.conf"),
            format!(
                "[DEFAULT]\nmain-repo = main\n\n\
                 [main]\nlocation = {}\n\n\
                 [overlay]\nlocation = {}\nmasters = main\n",
                main.display(),
                overlay.display(),
            ),
        )
        .unwrap();

        let ebuild_path = pkg_dir.join("overlaypkg-1.0.ebuild");
        let portage_tmpdir = tmp.join("portage_tmpdir");

        let status = run_commands(
            &ebuild_path,
            &["install"],
            Path::new("/"),
            &portage_tmpdir,
            &portage_tmpdir.join("distfiles"),
            false,
            &tmp,
            ShellBackend::Brush,
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should exit successfully");

        let marker = portage_tmpdir.join("portage/dev-libs/overlaypkg-1.0/temp/eclass-marker.txt");
        let observed = std::fs::read_to_string(&marker)
            .unwrap_or_else(|e| panic!("{} should have been written: {e}", marker.display()));
        assert_eq!(observed, "hello from mastershared.eclass\n");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn repo_root_for_finds_the_nearest_ancestor_repo_root() {
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-repo_root_for_finds",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = tmp.join("myrepo");
        let pkg_dir = repo.join("dev-libs/foo");
        std::fs::create_dir_all(repo.join("profiles")).unwrap();
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(repo.join("profiles/repo_name"), "myrepo\n").unwrap();
        assert_eq!(repo_root_for(&pkg_dir), Some(repo));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn repo_root_for_is_none_when_no_ancestor_has_one() {
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-repo_root_for_none",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("dev-libs/foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        assert_eq!(repo_root_for(&pkg_dir), None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Real, not simulated (task #56): passing `debug: true` really
    /// exports `PORTAGE_DEBUG=1` into the phase's own environment (see
    /// `run_one_phase`'s own setup block) -- proven here by having the
    /// fixture's own `src_install` record the value it actually observed,
    /// rather than asserting on captured `set -x` trace output (which
    /// would need redirecting the whole test process's stdout/stderr, a
    /// much heavier and flakier mechanism for the same underlying claim).
    /// Real `bin/ebuild.sh:479`'s own `[[ ${PORTAGE_DEBUG} == 1 ]]` guard
    /// is what turns this exported value into the real `set -x` xtrace a
    /// human running `ebuild <file> install --debug` directly would see;
    /// that guard itself is real, unmodified bash this pilot doesn't
    /// reimplement, so proving the export is correct is sufficient here.
    #[test]
    fn debug_flag_exports_real_portage_debug() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/debugpkg/debugpkg-1.0.ebuild");

        for (debug, expected) in [(true, "1"), (false, "0")] {
            let portage_tmpdir = std::env::temp_dir().join(format!(
                "ebuild-phases-test-{}-{}-{debug}",
                std::process::id(),
                "debug_flag_exports_real_portage_debug"
            ));
            let _ = std::fs::remove_dir_all(&portage_tmpdir);

            let status = run_commands(
                &ebuild_path,
                &["install"],
                Path::new("/"),
                &portage_tmpdir,
                &portage_tmpdir.join("distfiles"),
                debug,
                Path::new("/dev/null/no-config-root"),
                ShellBackend::Brush,
            )
            .expect("run_commands should not itself error");
            assert_eq!(status, 0);

            let marker =
                portage_tmpdir.join("portage/dev-libs/debugpkg-1.0/temp/portage-debug-value.txt");
            let observed = std::fs::read_to_string(&marker)
                .unwrap_or_else(|e| panic!("{} should have been written: {e}", marker.display()));
            assert_eq!(observed, expected, "debug={debug}");

            let _ = std::fs::remove_dir_all(&portage_tmpdir);
        }
    }

    /// `pretend` alone (the shortest real prerequisite chain -- see
    /// `phase_prerequisites`) still exercises the full real
    /// environment-setup + `bin/ebuild.sh`-sourcing + `__ebuild_main`
    /// path without needing `src_install` at all, proving the slice
    /// works even for an ebuild with no explicitly defined phases (every
    /// phase function `phasepkg` doesn't define comes from real EAPI
    /// defaults, ported here for free -- see this module's own doc
    /// comment).
    #[test]
    fn pretend_alone_succeeds_with_no_explicit_phase_functions() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "pretend_alone_succeeds_with_no_explicit_phase_functions"
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);

        let status = run_commands(
            &ebuild_path,
            &["pretend"],
            Path::new("/"),
            &portage_tmpdir,
            &portage_tmpdir.join("distfiles"),
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Brush,
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0);

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }
}
