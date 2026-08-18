// Real ebuild phase execution (task #54, PORTING/PROMPT-next.md's own
// "Real ebuild phase execution" section) -- the first slice: proving real
// phase functions run and real files land under a real `${D}`, without
// touching the vdb/CONTENTS/merge machinery at all (task #55, a
// separately-scoped, much bigger piece: `dblink.merge()`/`treewalk()`/
// `mergeme()` in `lib/portage/dbapi/vartree.py`, ~6500 lines).
//
// Bash-execution backend: an embedded `brush_core::Shell` (see
// `multicall/Cargo.toml`'s own doc comment for the pinned fork/commit and
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
//   - Only the `actionmap_deps`-chained phases run for real: `pretend`,
//     `setup`, `unpack`, `prepare`, `configure`, `compile`, `test`,
//     `install` (see `phase_prerequisites`'s own doc comment). Every
//     other real `ebuild` command (`merge`/`qmerge`/`unmerge`/`package`/
//     `preinst`/`postinst`/`prerm`/`postrm`/`config`/`info`/`nofetch`/
//     `depend`/`fetch`/`fetchall`/`digest`/`manifest`/`rpm`/`instprep`/
//     `clean`/`cleanrm`) still falls through to `ebuild.rs`'s own
//     pre-existing dry-run stub message unchanged.
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
    // multicall/src/ebuild_phases.rs -> multicall -> rust -> PORTING -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../")
        .canonicalize()
        .expect("repo root resolves (multicall is always built from within the real checkout)")
}

/// The real environment-variable block every real phase and every real
/// `bin/misc-functions.sh` `__dyn_*` command alike needs -- shared by
/// `run_one_phase` and `run_misc_function` so the two don't duplicate
/// this. `extra_env` is appended verbatim as more `export NAME=value`
/// lines (already-shell-quoted by the caller), for anything specific to
/// one call site (e.g. `ebuild_package`'s own `PKGDIR`/
/// `PORTAGE_BINPKG_TMPFILE`).
fn phase_setup_script(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    debug: bool,
    bin_dir: &Path,
    helpers_dir: &Path,
    extra_env: &[(String, String)],
) -> String {
    let mut script = format!(
        r#"
export EAPI={eapi:?}
export PN={pn:?}
export PV={pv:?}
export PR={pr:?}
export PVR={pvr:?}
export P={p:?}
export PF={pf:?}
export CATEGORY={category:?}
export EBUILD={ebuild:?}
export O={o:?}
export ROOT={root:?}
export EROOT={root:?}
export PORTAGE_BUILDDIR={builddir:?}
export WORKDIR={workdir:?}
export S={s:?}
export D={d:?}/
export ED="${{D}}"
export T={t:?}
export HOME={home:?}
export FILESDIR={filesdir:?}
export PORTAGE_BIN_PATH={bin_dir:?}
export PORTAGE_PYTHON=/usr/bin/python
export PATH={helpers_dir:?}:$PATH
export SANDBOX_DISABLED=1
export FEATURES=""
export USE=""
export EPREFIX=""
export EMERGE_FROM=ebuild
export PORTAGE_QUIET=1
export PORTAGE_DEBUG={portage_debug}
export EBUILD_PHASE={ebuild_phase_value:?}
"#,
        eapi = env.eapi,
        pn = env.split.pn,
        pv = env.split.pv,
        pr = env.split.pr,
        pvr = env.split.pvr,
        p = env.split.p,
        pf = env.split.pf,
        category = env.category,
        ebuild = env.ebuild_abs.display(),
        o = env.pkg_dir.display(),
        root = root.display(),
        builddir = env.portage_builddir.display(),
        workdir = env.workdir().display(),
        s = env.s().display(),
        d = env.d().display(),
        t = env.t().display(),
        home = env.home().display(),
        filesdir = env.filesdir().display(),
        bin_dir = bin_dir.display(),
        helpers_dir = helpers_dir.display(),
        portage_debug = if debug { "1" } else { "0" },
        ebuild_phase_value = ebuild_phase_value,
    );
    for (name, value) in extra_env {
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
async fn run_one_phase(
    env: &Environment,
    root: &Path,
    phase: &str,
    debug: bool,
) -> Result<i32, String> {
    let bin_dir = repo_root().join("bin");
    let helpers_dir = bin_dir.join("ebuild-helpers");

    let mut shell = brush_core::Shell::builder()
        .default_builtins(brush_builtins::BuiltinSet::BashMode)
        .build()
        .await
        .map_err(|e| format!("brush shell failed to start: {e}"))?;
    let params = shell.default_exec_params();

    let setup = phase_setup_script(env, root, phase, debug, &bin_dir, &helpers_dir, &[]);
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
async fn run_misc_functions(
    env: &Environment,
    root: &Path,
    ebuild_phase_value: &str,
    dyn_command: &str,
    extra_env: &[(String, String)],
    debug: bool,
) -> Result<i32, String> {
    let bin_dir = repo_root().join("bin");
    let helpers_dir = bin_dir.join("ebuild-helpers");

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
        &bin_dir,
        &helpers_dir,
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

/// Synchronous entry point mirroring `run_single_phase`'s own shape, for
/// `ebuild_package`'s own real `__dyn_package` call.
pub(crate) fn run_misc_function(
    ebuild_path: &Path,
    portage_tmpdir: &Path,
    root: &Path,
    ebuild_phase_value: &str,
    dyn_command: &str,
    extra_env: &[(String, String)],
    debug: bool,
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
async fn run_commands_async(
    ebuild_path: &Path,
    commands: &[&str],
    root: &Path,
    portage_tmpdir: &Path,
    debug: bool,
) -> Result<i32, String> {
    let env = compute_environment(ebuild_path, portage_tmpdir)?;
    create_directories(&env)?;

    for &command in commands {
        for phase in phase_prerequisites(command) {
            let status = run_one_phase(&env, root, phase, debug).await?;
            if status != 0 {
                return Ok(status);
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
pub fn run_commands(
    ebuild_path: &Path,
    commands: &[&str],
    root: &Path,
    portage_tmpdir: &Path,
    debug: bool,
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
        debug,
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
pub(crate) fn run_single_phase(
    ebuild_path: &Path,
    phase: &str,
    root: &Path,
    portage_tmpdir: &Path,
    debug: bool,
) -> Result<i32, String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to start async runtime: {e}"))?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        run_one_phase(&env, root, phase, debug).await
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
            false,
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
                debug,
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
            false,
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0);

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }
}
