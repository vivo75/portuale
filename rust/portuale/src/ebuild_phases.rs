// Real ebuild phase execution (task #54, docs/agent-context.md's own
// "Real ebuild phase execution" section) -- the first slice: proving real
// phase functions run and real files land under a real `${D}`, without
// touching the vdb/CONTENTS/merge machinery at all (task #55, a
// separately-scoped, much bigger piece: `dblink.merge()`/`treewalk()`/
// `mergeme()` in `lib/portage/dbapi/vartree.py`, ~6500 lines).
//
// Bash-execution backend: by default a genuine `bash` subprocess (real
// portage's own `_doebuild_spawn()` shape); optionally an embedded
// `brush_core::Shell` via `--shell brush` (see `portuale/Cargo.toml`'s
// own doc comment for the pinned commit, and `ShellBackend`'s doc
// comment for why bash is the default). Either way it drives the REAL,
// unmodified `bin/ebuild.sh` and the phase
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
// portuale's own `portage_dep`/`portage_profile` crates already
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
// KNOWN, DOCUMENTED GAPS (v1 scope, matching portuale's own
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
//   - The `FEATURES` build-isolation set **is** modelled (SCOPE_BACKLOG
//     Part 2.D): `sandbox`/`usersandbox`, `network-sandbox`,
//     `ipc-sandbox`, `mount-sandbox`, `pid-sandbox`. All apply to the
//     same six real `src_*` phases (`unpack`/`prepare`/`configure`/
//     `compile`/`test`/`install` -- real `_doebuild_spawn` sandboxes
//     every phase not in `_unsandboxed_phases` and unshares every phase
//     not in `_ipc_phases`; for the phases portuale runs as real bash
//     both come out to this set, `SANDBOXED_SRC_PHASES`). Any one of
//     them forces the `Bash` backend for those phases: neither an
//     `unshare(2)` namespace nor an LD_PRELOAD `libsandbox.so` can
//     confine the in-process `Brush` interpreter (the same constraint
//     the scheduler's captured builds accept). The wrappers compose --
//     `unshare <flags> --map-root-user -- sh -c '<config>; exec "$@"' _
//     [sandbox] bash bin/ebuild.sh <phase>` -- see `Isolation` /
//     `phase_isolation` / `sandbox_wrapped_command`.
//     * `FEATURES=sandbox` (or `usersandbox`): `sandbox bash …` (real
//       `spawn_sandbox`) when `/usr/bin/sandbox` exists (real
//       `sandbox_capable`). `phase_env_vars` sets
//       `SANDBOX_LOG=${T}/sandbox.log` (real `doebuild.py:526`) and
//       `SANDBOX_DISABLED=0` for the wrapped phase, so `bin/ebuild.sh`
//       does its own real `SANDBOX_ON=1` + `addread /` + `addwrite
//       "${PORTAGE_TMPDIR}/portage"` setup; the `sandbox` binary logs
//       any write outside the build tree and exits non-zero, failing
//       the phase. The `bin/misc-functions.sh` calls (`install_qa_check`
//       post-`install`, `__dyn_package`) are `sandbox`-wrapped too, with
//       a separate `SANDBOX_LOG=${T}/sandbox-misc.log` (real
//       `MiscFunctionsProcess._spawn`). A missing binary degrades to an
//       unsandboxed run with a one-shot warning (real `_spawn`'s own
//       silent `free = True` fallback).
//     * `FEATURES=network-sandbox` -> `unshare --net` + `ip link set lo
//       up` inside (real `_configure_loopback_interface`, minus its
//       `10.0.0.1/8` + `fd::1/8` `AI_ADDRCONFIG`-workaround addresses,
//       bug #690758). `FEATURES=ipc-sandbox` -> `unshare --ipc`.
//       `FEATURES=mount-sandbox` -> `unshare --mount` + `mount
//       --make-rslave /` inside (real `_exec2`). `FEATURES=pid-sandbox`
//       -> `unshare --pid --fork --mount-proc` (real `CLONE_NEWPID` +
//       `pid-ns-init`; `--fork` stands in for the full init).
//     * `RESTRICT=network-sandbox` / `PROPERTIES=live` (unpack) /
//       `PROPERTIES=test_network` (test) exemptions ARE real now
//       (`phase_isolation`'s own doc comment) -- the phase env carries
//       USE-reduced `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` too
//       (`restrict_and_properties`), the same real `doebuild_environment`
//       always sets, for real bash's own direct consumption
//       (`RESTRICT=test`/`RESTRICT=nostrip`/`RESTRICT=strip` skips).
//     * Cuts: SELinux sandbox (a kernel LSM feature with no meaning
//       outside a real SELinux-enabled host -- unlike the rest of this
//       set, there is no reasonable degrade to model, only a real
//       `libselinux`/policy dependency this scope has no use for);
//       `userpriv` / `fakeroot` -- these exist in real portage
//       specifically to drop privileges *from* an already-root process;
//       portuale never assumes root to begin with (see `ebuild_merge.rs`'s
//       own `os.lchown` cut, which needs root for the same reason), so
//       there's no privilege to drop and nothing for either feature to
//       do here; and, unlike real portage (which only unshares when
//       `uid == 0`), portuale always uses `--map-root-user` so it
//       works non-root -- an unavailable user namespace degrades with a
//       warning (real "Unable to unshare").
//   - `PORTAGE_PYM_PATH` (real portage's own Python-package import path)
//     is set to `<checkout>/lib` when the portage checkout exists (see
//     `phase_env_vars`'s own comment). It was originally left unset --
//     `create_directories` pre-creates `${PORTAGE_BUILDDIR}/empty` so
//     `bin/ebuild.sh`'s own "safe cwd" logic (EAPI 8's own comment:
//     "requires us to use an empty directory here") takes *that* branch
//     rather than `cd "${PORTAGE_PYM_PATH}" || die`, and it still does --
//     but the `bin/` helper scripts that `import portage`
//     (`portageq-wrapper`, `ebuild-pyhelper`, `save-ebuild-env.sh`) each
//     `cd "${PORTAGE_PYM_PATH}" || exit 1` unconditionally, so with it
//     unset every `has_version`/`best_version` an eclass runs failed.
//   - `__source_all_bashrcs` (real per-profile/package bashrc hook
//     support, `/etc/portage/bashrc` and friends) is left unimplemented
//     -- also observed as a non-fatal "command not found" warning, not a
//     phase failure. Portuale has no profile/make.conf-driven bashrc
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
//     portuale's own md5-cache-reading machinery (`portage-repo`'s own
//     `read_md5_cache`) -- real `ebuild <file> <command>` operates on an
//     arbitrary standalone ebuild file, not necessarily one that's part
//     of a configured, md5-cache-indexed repo, so this mirrors real
//     `_parse_eapi_ebuild_head` instead.
//   - `PORTAGE_TMPDIR` defaults to `/var/tmp/portage` (real portage's own
//     `make.globals` default) but is overridable via the `PORTAGE_TMPDIR`
//     environment variable -- portuale has no make.conf-reading path
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
/// bare `PF` string with no other information -- portuale's own
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
/// portuale's own CLI already handles separately (`wants_help`).
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
    portage_tmpdir: PathBuf,
    portage_builddir: PathBuf,
    /// Space-joined eclass names for `INHERITED` (real
    /// `porttree.py:872`'s `" ".join(mydata["_eclasses_"])`), from the
    /// ebuild's own repo `metadata/md5-cache` entry. `None` for a
    /// standalone ebuild outside any repo, or one that inherits nothing.
    /// Exported into every phase env so `bin/ebuild.sh`'s own
    /// `__INHERITED_QA_CACHE=${INHERITED}` snapshot suppresses the
    /// spurious `Eclass '…' inherited illegally` QA notice when a
    /// non-`depend` phase re-sources the ebuild -- see `phase_env_vars`.
    inherited: Option<String>,
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

    // Real `porttree.py:872`: `INHERITED = " ".join(_eclasses_)` -- the
    // eclass names from this ebuild's own repo `metadata/md5-cache`
    // entry, in order. Modern md5-cache stores `_eclasses_=<name>\t<md5>
    // \t<name>\t<md5>…`; older/fixture caches store a plain `INHERITED=
    // <space list>`. Absent for a standalone ebuild outside any repo.
    let inherited = repo_root_for(&pkg_dir)
        .and_then(|repo_root| portage_repo::read_md5_cache(&repo_root, &category, &split.pf).ok())
        .and_then(|md| {
            if let Some(eclasses) = md.get("_eclasses_") {
                let names: Vec<&str> = eclasses.split('\t').step_by(2).collect();
                (!names.is_empty()).then(|| names.join(" "))
            } else {
                md.get("INHERITED")
                    .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
                    .filter(|s| !s.is_empty())
            }
        });

    Ok(Environment {
        ebuild_abs,
        pkg_dir,
        category,
        split,
        eapi,
        portage_tmpdir: portage_tmpdir.to_path_buf(),
        portage_builddir,
        inherited,
    })
}

impl Environment {
    /// Real `${PORTAGE_BUILDDIR}` (`${PORTAGE_TMPDIR}/portage/<cat>/<pf>`)
    /// -- a per-merge scratch root the caller can hang its own temp
    /// subdirectories off (e.g. `ebuild_merge`'s replace-loop
    /// extracted-from-vdb ebuilds).
    pub(crate) fn portage_builddir(&self) -> &Path {
        &self.portage_builddir
    }
    /// Real `${PORTAGE_TMPDIR}` -- the root `bin/ebuild.sh`'s own
    /// `addwrite "${PORTAGE_TMPDIR}/portage"` opens to the sandbox, so
    /// it must be a real env var whenever `FEATURES=sandbox` is active.
    fn portage_tmpdir(&self) -> &Path {
        &self.portage_tmpdir
    }
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
    /// portuale writes nothing new for it, real phase execution already
    /// leaves it behind as a side effect (confirmed empirically: a real
    /// `ebuild <file> install` run via portuale's own binary leaves
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
/// `SRC_URI` archive -- portuale has no fetch/unpack machinery at all
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
        // (unset in portuale -- no Python-package-path concept at all)
        // otherwise, `die`-ing if neither works. Always pre-created here
        // so that fallback path, which portuale can't satisfy, is
        // never reached at all.
        env.portage_builddir.join("empty"),
        // Real `prepare_build_dirs` creates `${T}/logging`;
        // `bin/isolated-functions.sh::__elog_base` silently drops every
        // `elog`/`ewarn`/`eerror` message if the dir doesn't exist.
        env.t().join("logging"),
    ] {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

pub(crate) fn repo_root() -> PathBuf {
    // portuale/src/ebuild_phases.rs -> portuale -> rust -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .canonicalize()
        .expect("repo root resolves (portuale is always built from within the checkout)")
}

/// The gitignored working checkout of upstream Portage (`3rdparty/portage/`
/// by default, overridable with `$PORTUALE_PORTAGE_CHECKOUT`).
///
/// `portuale` vendors the whole *bash* phase runtime into `bin/`, but a
/// handful of pieces still come from here: the `.py` helpers that
/// `import portage` (`doins.py`, `xpak-helper.py`, …) and their
/// `lib/portage` import path, and `cnf/sets/portage.conf` (real
/// `--list-sets`). Absent when nobody cloned it -- callers degrade the
/// same way a missing binary already does. See `3rdparty/repos.toml` for
/// the pinned ref.
pub(crate) fn portage_checkout() -> PathBuf {
    if let Some(p) = std::env::var_os("PORTUALE_PORTAGE_CHECKOUT") {
        return PathBuf::from(p);
    }
    repo_root().join("3rdparty/portage")
}

/// The directory `PORTAGE_BIN_PATH` points at for real phase execution.
///
/// `bin/` (repo root) is a tracked, vendored copy of upstream Portage's
/// own `bin/` runtime -- all the `.sh` (`ebuild.sh` and its whole source
/// closure), every `ebuild-helpers/` script, `estrip`/`ecompress`, the
/// `*-qa-check.d/` sets and the stdlib-only `filter-bash-environment.py`
/// -- so `emerge` runs on a host with no Portage installed. Only the
/// `.py` helpers that `import portage` (`doins.py`, `xpak-helper.py`,
/// `gpkg-helper.py`, `dohtml.py`, `chmod-lite`, `xattr-helper.py`) are
/// not vendored; they need `lib/portage` and are still read from the
/// `portage_checkout()` tree when it exists.
///
/// So: when the checkout exists, `PORTAGE_BIN_PATH` is a symlink overlay
/// -- vendored `bin/` entries win, the not-vendored `.py` helpers fall
/// through to `<checkout>/bin/`. With no checkout it's the vendored
/// `bin/` directly (the `.py`-helper phases then degrade the same way a
/// missing binary already does).
///
/// Resolved once per process. `bin/ebuild.sh` only ever uses
/// `${PORTAGE_BIN_PATH}` as a literal string prefix for `source`, never
/// `realpath`s it, so a symlinked entry resolves to the vendored file.
pub(crate) fn bin_dir() -> &'static Path {
    use std::sync::OnceLock;
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let vendored = repo_root().join("bin");
        let checkout = portage_checkout().join("bin");
        if !checkout.is_dir() {
            return vendored;
        }
        let overlay = std::env::temp_dir().join(format!("portuale-bin.{}", std::process::id()));
        match build_bin_overlay(&overlay, &checkout, &vendored) {
            Ok(()) => overlay,
            Err(e) => {
                eprintln!(
                    "portuale: bin/ overlay setup failed ({e}); \
                     falling back to {} (vendored bin/ changes not applied)",
                    checkout.display()
                );
                checkout
            }
        }
    })
    .as_path()
}

/// Populates `overlay` with a symlink to every entry of `checkout`, then
/// symlinks every entry of `vendored` over the top (dropping the
/// checkout link first). Rebuilt from scratch each call.
fn build_bin_overlay(overlay: &Path, checkout: &Path, vendored: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::symlink;
    let _ = std::fs::remove_dir_all(overlay);
    std::fs::create_dir_all(overlay)?;
    for entry in std::fs::read_dir(checkout)? {
        let entry = entry?;
        symlink(entry.path(), overlay.join(entry.file_name()))?;
    }
    for entry in std::fs::read_dir(vendored)? {
        let entry = entry?;
        let dest = overlay.join(entry.file_name());
        let _ = std::fs::remove_file(&dest);
        symlink(entry.path(), dest)?;
    }
    Ok(())
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
/// fetched filename list (portuale's own always-empty USE set, see
/// `crate::fetch::fetch_src_uri`'s own doc comment); `AA` is every
/// filename `SRC_URI` could ever reference regardless of USE (real
/// PMS's own definition), computed but never itself fetched.
/// Real `RESTRICT=mirror` (real `PORTAGE_RESTRICT`, and `fetch.py:880` --
/// the deprecated negative `nomirror` counts too). The md5-cache
/// `RESTRICT` field is the raw ebuild value, so it's USE-conditional-
/// evaluated first (real `_PackageMetadataWrapper`'s own `use_reduce`
/// pass, same as `PROPERTIES`/`LICENSE`) against portuale's own
/// always-empty fetch-side USE set (see `fetch::fetch_src_uri`'s own doc
/// comment) -- so every `foo? ( … )` group drops and only an
/// unconditional `mirror`/`nomirror` counts. An unparsable value yields
/// `false` (the "can't tell, so don't claim it" precedent).
pub(crate) fn restrict_mirror_from_restrict(restrict: &str) -> bool {
    flat_field_has_token(restrict, &["mirror", "nomirror"])
}

/// Real `RESTRICT=fetch` (`fetch.py:1061`, `restrict_fetch = "fetch" in
/// restrict`), USE-conditional-evaluated the same way
/// `restrict_mirror_from_restrict` does. Gates the plain-`SRC_URI`-URI
/// and public-`GENTOO_MIRRORS` candidates in
/// `crate::fetch::fetch_src_uri` (see `FetchOptions::restrict_fetch`).
pub(crate) fn restrict_fetch_from_restrict(restrict: &str) -> bool {
    flat_field_has_token(restrict, &["fetch"])
}

/// USE-conditional-evaluates a raw md5-cache `RESTRICT`/`PROPERTIES`
/// value (real `_PackageMetadataWrapper`'s own `use_reduce` pass)
/// against portuale's own always-empty phase-side USE set (see
/// `restrict_and_properties`'s own doc comment for why), then checks
/// whether any of `wanted` survived. Shared by every `RESTRICT`/
/// `PROPERTIES` single-token check in this module -- the field doesn't
/// matter to the tokenizing/reducing/matching logic itself, only to the
/// caller's own choice of `wanted`.
fn flat_field_has_token(raw: &str, wanted: &[&str]) -> bool {
    if raw.trim().is_empty() {
        return false;
    }
    let tokens: Vec<String> = raw.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat(
        &tokens,
        &std::collections::HashSet::new(),
        portage_use_reduce::MatchMode::Normal,
    )
    .map(|flat| flat.iter().any(|t| wanted.contains(&t.as_str())))
    .unwrap_or(false)
}

/// USE-conditional-evaluates a raw `RESTRICT`/`PROPERTIES` value the
/// same way `flat_field_has_token` does, but returns the whole flattened
/// token list joined back into a plain-text string -- real portage's
/// own `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` env vars carry exactly
/// this shape (`doebuild_environment()`'s own `str(self._pkg.restrict)`/
/// `str(self._pkg.properties)`, both already USE-reduced `_pkg`
/// accessors -- real bash's own `contains_word … "${PORTAGE_RESTRICT}"`
/// checks, e.g. `phase-functions.sh:549`'s `RESTRICT=test` skip and
/// `:777`'s `RESTRICT=nostrip`/`RESTRICT=strip`, consume the reduced
/// form, not the raw ebuild one). An unparsable value degrades to `""`,
/// the same "can't tell, so don't claim it" precedent
/// `flat_field_has_token` already uses.
fn flat_field(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let tokens: Vec<String> = raw.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat(
        &tokens,
        &std::collections::HashSet::new(),
        portage_use_reduce::MatchMode::Normal,
    )
    .map(|flat| flat.join(" "))
    .unwrap_or_default()
}

/// Real `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` (`doebuild_environment()`
/// sets both, unconditionally, for every phase): the ebuild's own
/// `RESTRICT`/`PROPERTIES` metadata, USE-reduced. Read from the same
/// repo's own `metadata/md5-cache` entry `fetch_sources`'s own `RESTRICT`
/// read already trusts, against portuale's own always-empty phase-side
/// USE set -- no resolved graph reaches a standalone `ebuild <file>
/// <phase>`, and `entry_build_env`'s own resolved USE (an `emerge -b`
/// build) doesn't reach this deep yet either (see this module's own
/// "KNOWN, DOCUMENTED GAPS"). `("", "")` outside any repo checkout,
/// matching `repo_root_for`'s own established tolerance.
fn restrict_and_properties(env: &Environment) -> (String, String) {
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return (String::new(), String::new());
    };
    let metadata = portage_repo::read_md5_cache(&repo_root, &env.category, &env.split.pf).ok();
    let get = |key: &str| {
        metadata
            .as_ref()
            .and_then(|m| m.get(key))
            .map(String::as_str)
            .unwrap_or("")
    };
    (flat_field(get("RESTRICT")), flat_field(get("PROPERTIES")))
}

#[allow(clippy::too_many_arguments)]
async fn fetch_sources(
    env: &Environment,
    root: &Path,
    distdir: &Path,
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
) -> Result<(Vec<String>, Vec<String>), String> {
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let metadata = portage_repo::read_md5_cache(&repo_root, &env.category, &env.split.pf).ok();
    let src_uri = metadata
        .as_ref()
        .and_then(|m| m.get("SRC_URI").cloned())
        .unwrap_or_default();
    if src_uri.trim().is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let restrict = metadata.as_ref().and_then(|m| m.get("RESTRICT"));
    let restrict_mirror = restrict
        .map(|r| restrict_mirror_from_restrict(r))
        .unwrap_or(false);
    let restrict_fetch = restrict
        .map(|r| restrict_fetch_from_restrict(r))
        .unwrap_or(false);
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
            restrict_mirror,
            restrict_fetch,
        },
    );
    let a = match a {
        Ok(a) => a,
        Err(e) => {
            // Real `fetch.py`: when a distfile can't be fetched, run the
            // ebuild's own `pkg_nofetch` phase -- it prints custom "get it
            // from <URL> and drop it in <DISTDIR>" instructions -- then
            // fail. Best-effort: the phase not being defined (or its own
            // failure) never masks the real fetch error.
            let ebuild = env.pkg_dir.join(format!("{}.ebuild", env.split.pf));
            if ebuild.is_file() {
                let _ =
                    run_one_phase(env, root, "nofetch", debug, &[], config_root, shell, None).await;
            }
            return Err(e);
        }
    };
    Ok((a, aa))
}

/// Real `doebuild.py::_post_src_install_write_metadata`
/// (`lib/portage/package/ebuild/doebuild.py:2700-2782`), run right after
/// a successful `src_install` (real portage calls it from
/// `doebuild(mydo="install")`): write the USE-conditional-evaluated
/// dependency / `LICENSE` / `PROPERTIES` / `RESTRICT` / `IUSE`
/// (+ `IUSE_EFFECTIVE`) metadata files into `${PORTAGE_BUILDDIR}/build-
/// info`. `bin/phase-functions.sh __dyn_install`'s own build-info loop
/// (run unmodified, before this) writes `CATEGORY`/`SLOT`/`KEYWORDS`/
/// `IUSE`/`USE`/`EAPI`/`DEFINED_PHASES`/… but *not* these keys -- real
/// portage's Python side fills them in. Without this the merged vdb
/// entry and the `xpak`/`gpkg` a `$PKGDIR` scan later reads carry no
/// dependency metadata at all (found via the `binpkg.rs` scan buildout).
///
/// Source is the ebuild's own `metadata/md5-cache` entry (the same
/// already-trusted source `fetch_sources` reads `SRC_URI` from) --
/// `settings.configdict["pkg"]` in real portage. USE-conditionals are
/// evaluated against `use_flags` -- the resolved `USE` for this package
/// (`build_phase_use` pulls it out of the `emerge <atom>` build path's
/// own `build_env`; empty for a standalone `ebuild <file>` run, which
/// resolves no graph) -- via `use_reduce_structured` (real
/// `paren_enclose(use_reduce(v, uselist=use))`, the bracket/`||`-
/// preserving normalized token stream).
///
/// Real `_slot_operator._eval_deps`'s own per-atom step: an atom with a
/// `:=` slot operator (`slot_operator == "="`) is rewritten to
/// `:<slot>/<sub-slot>=` taken from the highest installed version in
/// `<root>/var/db/pkg` that satisfies it (`vardb.match(x)[-1]`). A
/// non-atom token, a non-`:=` atom, or a `:=` dep with nothing installed
/// is returned unchanged (real "just leave it as-is for now ... keeping
/// the information in vdb").
///
/// The rewrite is string surgery on the atom's own slot-dep substring
/// (`:=` / `:2=` / `:2/3=`, all reconstructable from the parsed
/// `slot`/`sub_slot`) rather than reserialising the whole atom -- that
/// substring is distinctive enough to appear exactly once in a
/// well-formed atom, so `replacen(.., 1)` is safe (a version can't
/// contain `:`, `::repo` carries no `=`, a `[usedep]` carries no `:`).
fn bind_slot_operator(token: &str, root: &Path) -> String {
    let Some(atom) = portage_dep::parse_atom(token) else {
        return token.to_string();
    };
    if atom.slot_operator != Some(portage_dep::SlotOperator::Equals) {
        return token.to_string();
    }
    let best = portage_repo::installed_candidates(root, &atom.category, &atom.package)
        .into_iter()
        .filter(|(version, slot, sub_slot)| {
            let cpv_slot = format!(
                "{}/{}-{version}:{slot}/{sub_slot}",
                atom.category, atom.package
            );
            portage_dep::match_from_list(token, &[cpv_slot.as_str()]).is_some_and(|m| !m.is_empty())
        })
        .max_by(|(a, _, _), (b, _, _)| {
            portage_versions::vercmp(a, b)
                .map(|c| c.cmp(&0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    let Some((_, slot, sub_slot)) = best else {
        return token.to_string();
    };
    let old_slotdep = match (&atom.slot, &atom.sub_slot) {
        (None, _) => ":=".to_string(),
        (Some(s), None) => format!(":{s}="),
        (Some(s), Some(ss)) => format!(":{s}/{ss}="),
    };
    token.replacen(&old_slotdep, &format!(":{slot}/{sub_slot}="), 1)
}

/// Real portage, for an EAPI with slot operators (every EAPI 5+), skips
/// the `*DEPEND` keys in this loop and writes them from
/// `evaluate_slot_operator_equal_deps` (`portage/dep/_slot_operator.py`)
/// instead: every `:=` slot-operator atom is bound to the actual
/// `<slot>/<sub-slot>=` of the highest installed version that satisfies
/// it (`vardb.match(x)[-1]`), leaving an unresolvable one bare. This
/// portuale now does the same, `bind_slot_operator` per `*DEPEND` token
/// (`_eval_deps`'s own per-atom loop) -- so a package portuale merges
/// records `dev-libs/foo:2/3=` in its vdb/binpkg build-info, the data a
/// later sub-slot rebuild check needs. An ebuild with no `:=` operator,
/// or one whose `:=` dep isn't installed, is byte-identical to before.
///
/// v1 cut still: real `_eval_deps` walks `RDEPEND`/`PDEPEND` against the
/// target `ROOT` vdb and `DEPEND`/`BDEPEND` against the target/running
/// vdb respectively -- portuale's own single-root world binds every
/// `*DEPEND` key against the one `<root>/var/db/pkg` (same simplification
/// `--root-deps` documents); and the real `|| ( A:= B:= )` "record
/// sub-slot on A only" TODO (bug #455904) is moot without disjunctive
/// `:=` handling anywhere.
/// The `USE=` value from a `run_commands_async` `build_env` slice as a
/// flag set -- what the `emerge <atom>` build path resolved for this
/// package. Empty for a standalone `ebuild <file>` run (no `build_env`).
fn build_phase_use(build_env: &[(String, String)]) -> std::collections::HashSet<String> {
    build_env
        .iter()
        .find(|(k, _)| k == "USE")
        .map(|(_, v)| v.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

fn write_post_install_metadata(
    env: &Environment,
    root: &Path,
    use_flags: &std::collections::HashSet<String>,
) -> Result<(), String> {
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return Ok(());
    };
    let Ok(metadata) = portage_repo::read_md5_cache(&repo_root, &env.category, &env.split.pf)
    else {
        return Ok(());
    };
    let build_info = env.build_info();

    // real `_vdb_use_conditional_keys` = `Package._dep_keys` + LICENSE /
    // PROPERTIES / RESTRICT.
    for key in [
        "DEPEND",
        "RDEPEND",
        "BDEPEND",
        "PDEPEND",
        "IDEPEND",
        "LICENSE",
        "PROPERTIES",
        "RESTRICT",
    ] {
        let Some(raw) = metadata
            .get(key)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        else {
            // real portage unlinks a stale build-info/<k> when the value
            // is empty; `bin/phase-functions.sh` never wrote these, so
            // there is nothing to unlink here.
            continue;
        };
        let tokens: Vec<String> = raw.split_whitespace().map(String::from).collect();
        let reduced = portage_use_reduce::use_reduce_structured(
            &tokens,
            use_flags,
            portage_use_reduce::MatchMode::Normal,
        )
        .map_err(|e| format!("{}: build-info/{key}: {e}", env.pkg_dir.display()))?;
        // Real `_post_src_install_write_metadata`: the `*DEPEND` keys go
        // through `evaluate_slot_operator_equal_deps` -- bind every `:=`
        // atom to the installed dependency's `<slot>/<sub-slot>=`.
        let value = if key.ends_with("DEPEND") {
            reduced
                .into_iter()
                .map(|tok| bind_slot_operator(&tok, root))
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            reduced.join(" ")
        };
        if value.is_empty() {
            continue;
        }
        std::fs::write(build_info.join(key), format!("{value}\n"))
            .map_err(|e| format!("{}: {e}", build_info.join(key).display()))?;
    }

    // real: `settings.configdict["pkg"]["IUSE"]` written verbatim ("in
    // case it's corrupted due to local environment settings", bug
    // #386829) -- `bin/phase-functions.sh` already wrote it, but only
    // when non-empty; re-assert from md5-cache so it is always present
    // and canonical. `IUSE_EFFECTIVE` is the profile's own EAPI 5+
    // `_calc_iuse_effective` result -- portuale computes it as
    // `Config::iuse_effective`, not reachable from here without threading
    // a resolved `Config` through the whole phase chain; left as a
    // documented gap (the vdb `IUSE_EFFECTIVE` file is only read by a
    // built package's own USE-dep check, itself already narrowed -- see
    // `dependency_avoid_update_candidate`).
    if let Some(iuse) = metadata
        .get("IUSE")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        std::fs::write(build_info.join("IUSE"), format!("{iuse}\n"))
            .map_err(|e| format!("{}: {e}", build_info.join("IUSE").display()))?;
    }
    Ok(())
}

/// Which real shell executes a phase, and every real `bin/*.sh` this
/// portuale sources unmodified along with it.
///
/// `Bash` (**the default**) is a genuine `bash <bin_dir>/ebuild.sh
/// <phase>` subprocess -- matching real portage's own `_doebuild_spawn()`
/// invocation shape almost exactly (`lib/portage/package/ebuild/
/// doebuild.py`'s own `cmd = "{ebuild.sh} {phase}"`, spawned via
/// `portage.process.spawn()`; real `bin/ebuild.sh:153`'s own
/// `EBUILD_SH_ARGS="$*"` picks up `<phase>` from the subprocess's own
/// positional args, which its own tail, `bin/ebuild.sh:830-843`, then
/// really uses to call `__ebuild_main ${EBUILD_SH_ARGS}` and `exit`).
///
/// `Brush` is an embedded `brush_core::Shell` (see this module's own doc
/// comment, and `portuale/Cargo.toml`'s, for the pinned commit and how
/// the embedding works). It deliberately never sets `EBUILD_SH_ARGS`,
/// since a bare `exit` inside an *embedded* shell would kill the whole
/// hosting Rust process rather than just return control -- so it uses
/// brush's own "source, then separately `invoke_function`" two-step
/// instead of `__ebuild_main`.
///
/// **Why `Bash` is the default** (it was `Brush` originally, for the
/// zero-dependency / minimal-Linux fit -- hard goal 3): brush's `declare
/// -f` function serializer corrupts any function body containing a
/// redirected here-document (the redirect is torn off the `cat` line and
/// re-emitted after the body with its `"${var}"` target mangled; a `<<-`
/// body is re-indented with spaces so its terminator no longer matches).
/// `__save_ebuild_env` runs `declare -f` on every in-scope function
/// between phases, and `toolchain-funcs.eclass`'s `_tc-has-openmp` (plus
/// others) trips this -- the written `${T}/environment` then fails to
/// parse and the next phase's `source "${T}/environment" || die` aborts
/// the build. That breaks a real `emerge <atom>` for essentially every
/// compiled package. `Bash` has no such problem; `Brush` stays available
/// via `--shell brush` / `--shell=brush` (a portuale-only flag on both
/// `emerge` and `ebuild`, deliberately NOT in `ebuild_options::OPTIONS`
/// -- that table transcribes real `bin/ebuild`'s argparse only). The
/// brush `declare -f` bug is tracked in `docs/brush-pin.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellBackend {
    #[default]
    Bash,
    Brush,
}

/// The real `src_*` phases portuale puts inside a sandbox -- both the
/// `FEATURES=network-sandbox` net namespace and the `FEATURES=sandbox`
/// `sys-apps/sandbox` filesystem confinement apply to exactly this set.
/// Real portage: `_doebuild_spawn` sandboxes every phase not in
/// `_unsandboxed_phases` (`clean`/`config`/`setup`/`pre|post*`/…), and
/// network-unshares every phase not in `_ipc_phases`; for the phases
/// portuale actually runs as real bash both come out to this list.
/// `nofetch` is deliberately excluded (real `spawn_nofetch` uses its own
/// private tmpdir and neither unshares nor `sandbox`-wraps).
const SANDBOXED_SRC_PHASES: &[&str] = &[
    "unpack",
    "prepare",
    "configure",
    "compile",
    "test",
    "install",
];

/// A `FEATURES` token check, the same one `pretend.rs`/`ebuild_merge.rs`
/// already do -- read straight from the process environment.
fn feature_token_present(token: &str) -> bool {
    std::env::var("FEATURES")
        .map(|f| f.split_whitespace().any(|t| t == token))
        .unwrap_or(false)
}

/// `FEATURES=network-sandbox` present?
fn network_sandbox_requested() -> bool {
    feature_token_present("network-sandbox")
}

/// Real `_doebuild_spawn`'s own `networked` exemption formula
/// (`doebuild.py:241-251`), the half of `phase_isolation`'s own
/// network-unshare decision that doesn't depend on `FEATURES` itself
/// (that part is `network_sandbox_requested()`, a process-env read
/// tests can't safely mutate in parallel -- see `run_commands`'s own
/// doc comment) -- kept as its own pure function purely so it can be
/// unit-tested directly. `restrict`/`properties` are already
/// USE-reduced flat token strings (`restrict_and_properties`). `true`
/// when: `phase == "unpack"` and the ebuild's own `PROPERTIES` says
/// `live` (a live/VCS package's checkout step needs the network by
/// definition); `phase == "test"` and `PROPERTIES` says `test_network`
/// (an ebuild that declares its own test suite needs network access);
/// or the ebuild's own `RESTRICT` says `network-sandbox` (an explicit
/// per-package opt-out of the whole feature, regardless of phase).
fn network_sandbox_exempt(phase: &str, restrict: &str, properties: &str) -> bool {
    (phase == "unpack" && flat_field_has_token(properties, &["live"]))
        || (phase == "test" && flat_field_has_token(properties, &["test_network"]))
        || flat_field_has_token(restrict, &["network-sandbox"])
}

/// `FEATURES=sandbox` or `FEATURES=usersandbox` present -- real
/// `_spawn`'s own `"sandbox" not in features and "usersandbox" not in
/// features` gate. (Portuale does no `userpriv`, so `sandbox` and
/// `usersandbox` are equivalent here.)
fn fs_sandbox_requested() -> bool {
    feature_token_present("sandbox") || feature_token_present("usersandbox")
}

/// Real `portage.const.SANDBOX_BINARY` (`/usr/bin/sandbox`), and real
/// `portage.process.sandbox_capable` (the file exists and is
/// executable). `None` -> `FEATURES=sandbox` silently degrades to an
/// unsandboxed run, exactly real `_spawn`'s own
/// `if not free and not (fakeroot or sandbox_capable): free = True`.
fn sandbox_binary() -> Option<&'static Path> {
    use std::sync::OnceLock;
    static BIN: OnceLock<Option<PathBuf>> = OnceLock::new();
    BIN.get_or_init(|| {
        let p = PathBuf::from("/usr/bin/sandbox");
        // `X_OK` is good enough -- matches real `os.access(_, os.X_OK)`.
        if p.is_file()
            && std::fs::metadata(&p)
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt as _;
                    m.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false)
        {
            Some(p)
        } else {
            None
        }
    })
    .as_deref()
}

/// Real `FEATURES=sandbox` for one phase: `true` when the feature is
/// requested, the phase is a sandboxed `src_*` phase, and the `sandbox`
/// binary is available. A single warning is printed (real `_spawn`'s own
/// silent degrade is matched with an explicit note) when the feature is
/// on but the binary is missing.
fn fs_sandbox_for_phase(phase: &str) -> bool {
    use std::sync::OnceLock;
    if !fs_sandbox_requested() || !SANDBOXED_SRC_PHASES.contains(&phase) {
        return false;
    }
    if sandbox_binary().is_some() {
        return true;
    }
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        eprintln!(
            "!!! /usr/bin/sandbox not found (for FEATURES=\"sandbox\"); \
             src_* phases run without filesystem confinement"
        );
    });
    false
}

/// Every namespace/confinement wrapper to apply to one phase's real bash
/// subprocess -- real `_doebuild_spawn`'s `unshare_{net,ipc,mount,pid}`
/// + `spawn_sandbox`, collapsed to what portuale models.
#[derive(Clone, Copy, Default)]
struct Isolation {
    /// `FEATURES=network-sandbox` -> `unshare --net` (real `CLONE_NEWNET`).
    net: bool,
    /// `FEATURES=ipc-sandbox` -> `unshare --ipc` (real `CLONE_NEWIPC`).
    ipc: bool,
    /// `FEATURES=mount-sandbox` -> `unshare --mount` + `mount --make-rslave /`
    /// (real `CLONE_NEWNS` + real `_exec2`'s own `mount --make-rslave /`).
    mount: bool,
    /// `FEATURES=pid-sandbox` -> `unshare --pid --fork --mount-proc`
    /// (real `CLONE_NEWPID` + `pid-ns-init`; `unshare --fork` stands in
    /// for the full init, `--mount-proc` for real `_exec2`'s own new
    /// `/proc` mount).
    pid: bool,
    /// `FEATURES=sandbox`/`usersandbox` -> `sandbox <cmd>` (real
    /// `spawn_sandbox`).
    fs_sandbox: bool,
}

impl Isolation {
    fn any_unshare(&self) -> bool {
        self.net || self.ipc || self.mount || self.pid
    }
    fn any(&self) -> bool {
        self.any_unshare() || self.fs_sandbox
    }
    /// The `unshare(1)` flags for this combination (always with
    /// `--map-root-user` first, so it works from portuale's non-root
    /// context -- real portage only unshares when `uid == 0`).
    fn unshare_flags(&self) -> Vec<&'static str> {
        let mut f = vec!["--map-root-user"];
        if self.net {
            f.push("--net");
        }
        if self.ipc {
            f.push("--ipc");
        }
        if self.mount {
            f.push("--mount");
        }
        if self.pid {
            f.extend(["--pid", "--fork", "--mount-proc"]);
        }
        f
    }
}

/// Whether `unshare <flags> -- true` actually succeeds here (cached per
/// distinct flag combination). Real portage validates the `unshare(2)`
/// call in a short-lived subprocess before relying on it
/// (`_unshare_validator`); this is the same idea via the `unshare(1)`
/// CLI, which portuale already assumes is present the way it assumes
/// `tar`/`wget`/`bash`. A `false` result (unprivileged user namespaces
/// disabled, or no `unshare` binary) drops the wrappers with a warning
/// -- real portage's own non-fatal "Unable to unshare" degrade.
fn unshare_combo_usable(flags: &[&str]) -> bool {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let key = flags.join(" ");
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(&v) = cache.lock().unwrap().get(&key) {
        return v;
    }
    let ok = std::process::Command::new("unshare")
        .args(flags)
        .args(["--", "true"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    cache.lock().unwrap().insert(key, ok);
    ok
}

/// The isolation wrappers to apply to one phase, from `FEATURES` -- real
/// `_doebuild_spawn`. Only the `src_*` phases (`SANDBOXED_SRC_PHASES`)
/// are wrapped; a requested-but-unusable `unshare` combination degrades
/// to no unshare with one warning (real "Unable to unshare").
///
/// `net` also carries real `_doebuild_spawn`'s own `networked` exemption
/// formula (`doebuild.py:241-251`): `FEATURES=network-sandbox` is
/// requested but this call still isn't network-unshared when `phase ==
/// "unpack"` and the ebuild's own `PROPERTIES` says `live` (a live/VCS
/// package's checkout step needs the network by definition), when
/// `phase == "test"` and `PROPERTIES` says `test_network` (an ebuild
/// that declares its own test suite needs network access), or when the
/// ebuild's own `RESTRICT` says `network-sandbox` (an explicit ebuild
/// opt-out of the whole feature, regardless of phase). `_ipc_phases`
/// (`setup`/`pretend`/`config`/`info`/`pre|postinst`/`pre|postrm`) is
/// real's own third exemption clause, but every one of those is already
/// outside `SANDBOXED_SRC_PHASES` here, so it never needs its own check.
fn phase_isolation(env: &Environment, phase: &str) -> Isolation {
    use std::sync::OnceLock;
    if !SANDBOXED_SRC_PHASES.contains(&phase) {
        return Isolation::default();
    }
    let mut net = network_sandbox_requested();
    if net {
        let (restrict, properties) = restrict_and_properties(env);
        if network_sandbox_exempt(phase, &restrict, &properties) {
            net = false;
        }
    }
    let mut iso = Isolation {
        net,
        ipc: feature_token_present("ipc-sandbox"),
        mount: feature_token_present("mount-sandbox"),
        pid: feature_token_present("pid-sandbox"),
        fs_sandbox: fs_sandbox_for_phase(phase),
    };
    if iso.any_unshare() && !unshare_combo_usable(&iso.unshare_flags()) {
        static WARNED: OnceLock<()> = OnceLock::new();
        WARNED.get_or_init(|| {
            eprintln!(
                "!!! Unable to unshare (for FEATURES=\"network-sandbox / ipc-sandbox / \
                 mount-sandbox / pid-sandbox\"); src_* phases run without namespace isolation"
            );
        });
        iso.net = false;
        iso.ipc = false;
        iso.mount = false;
        iso.pid = false;
    }
    iso
}

/// Build a blocking `Command` that runs `bash <script> <arg>` (real
/// `_doebuild_spawn`'s own `EBUILD_SH_BINARY <arg>` shape) wrapped per
/// `iso`:
///
///   `unshare <flags> -- sh -c '<config>; exec "$@"' _ [sandbox] bash <script> <arg>`
///
/// - `FEATURES=sandbox` prepends the `sys-apps/sandbox` binary (real
///   `spawn_sandbox`: `args = [SANDBOX_BINARY, mycommand]`). `bin/*.sh`
///   itself does the `SANDBOX_ON=1` / `addread /` / `addwrite
///   "${PORTAGE_TMPDIR}/portage"` setup (given `SANDBOX_LOG` /
///   `SANDBOX_DISABLED=0` from `phase_env_vars`); `sandbox` exits
///   non-zero once its log gains a violation.
/// - `FEATURES={network,ipc,mount,pid}-sandbox` wrap the command in the
///   matching `unshare(1)` namespaces (real `_exec`'s
///   `unshare(CLONE_NEW*)`). The `sh -c` shim configures what real
///   `_exec2` configures inside each namespace -- `ip link set lo up`
///   for `--net` (real `_configure_loopback_interface`, minus the
///   `AI_ADDRCONFIG` addresses), `mount --make-rslave /` for `--mount`
///   (real `_exec2`'s own call) -- then `exec "$@"` runs the real
///   (possibly `sandbox`-prefixed) command.
fn sandbox_wrapped_command(script: &Path, arg: &str, iso: Isolation) -> std::process::Command {
    use std::ffi::OsString;

    let mut argv: Vec<OsString> = Vec::new();
    if iso.fs_sandbox {
        // Presence was already confirmed by `fs_sandbox_for_phase` /
        // the misc-functions caller before `iso.fs_sandbox` was set.
        if let Some(bin) = sandbox_binary() {
            argv.push(bin.into());
        }
    }
    argv.push("bash".into());
    argv.push(script.into());
    argv.push(arg.into());

    if !iso.any_unshare() {
        let mut c = std::process::Command::new(&argv[0]);
        c.args(&argv[1..]);
        return c;
    }

    let mut shim = String::new();
    if iso.net {
        shim.push_str("ip link set lo up 2>/dev/null; ");
    }
    if iso.mount {
        shim.push_str("mount --make-rslave / 2>/dev/null; ");
    }
    shim.push_str("exec \"$@\"");

    let mut c = std::process::Command::new("unshare");
    c.args(iso.unshare_flags())
        .arg("--")
        .arg("sh")
        .arg("-c")
        .arg(shim)
        .arg("portuale-sandbox")
        .args(&argv);
    c
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
/// `inherit ...` line -- previously portuale never populated this
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
        // Real `doebuild.py:543`: `PORTAGE_COLORMAP = colormap()` -- bash
        // source `bin/isolated-functions.sh` `eval`s so an ebuild's
        // `elog`/`einfo` use the same (`color.map`-aware) colours.
        (
            "PORTAGE_COLORMAP".to_string(),
            crate::color::phase_colormap_export(),
        ),
        ("PATH".to_string(), path),
        (
            "PORTAGE_TMPDIR".to_string(),
            env.portage_tmpdir().display().to_string(),
        ),
        // Real `doebuild.py:526`: always set for a non-`depend` phase.
        // `bin/phase-functions.sh` deletes a stale one before each phase
        // and (for `FEATURES=sandbox`) `sandbox` appends violations to it.
        (
            "SANDBOX_LOG".to_string(),
            env.t().join("sandbox.log").display().to_string(),
        ),
        // `bin/phase-functions.sh`: `[[ ${SANDBOX_DISABLED:-0} = 0 ]] &&
        // export SANDBOX_ON=1` for the `src_*` phases. `"0"` only when
        // this phase is actually `sandbox`-wrapped (see
        // `fs_sandbox_for_phase` / `run_one_phase`), so the unwrapped
        // default stays exactly as before.
        (
            "SANDBOX_DISABLED".to_string(),
            if fs_sandbox_for_phase(ebuild_phase_value) {
                "0"
            } else {
                "1"
            }
            .to_string(),
        ),
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

    // Real `doebuild_environment()`: `PORTAGE_RESTRICT`/
    // `PORTAGE_PROPERTIES` are set for every phase, always, from the
    // package's own already-USE-reduced `RESTRICT`/`PROPERTIES` (real
    // `str(self._pkg.restrict)`/`str(self._pkg.properties)`). Real bash
    // consults these directly -- `phase-functions.sh:549`'s own
    // `RESTRICT=test` skip, `:777`'s `RESTRICT=nostrip`/
    // `RESTRICT=strip` -- rather than re-deriving them from `RESTRICT`/
    // `PROPERTIES` itself, which portuale's own phase env never exports
    // at all (only the ebuild's own bash sees the raw, unreduced values,
    // via its own sourced metadata). See `restrict_and_properties`'s own
    // doc comment for the exact real source and portuale's own
    // USE-reduction narrowing.
    let (restrict, properties) = restrict_and_properties(env);
    vars.push(("PORTAGE_RESTRICT".to_string(), restrict));
    vars.push(("PORTAGE_PROPERTIES".to_string(), properties));

    // Real `INHERITED` (`porttree.py:872`): exported into every phase so
    // that when a non-`depend` phase re-sources the ebuild,
    // `bin/ebuild.sh`'s `__INHERITED_QA_CACHE=${INHERITED}` snapshot (then
    // `unset INHERITED`, then `source "${EBUILD}"`) lets the re-run
    // `inherit` calls find every eclass already known -- suppressing the
    // spurious `QA Notice: Eclass '…' inherited illegally in … <phase>`
    // real portage never emits here. See `Environment::inherited`.
    if let Some(inherited) = &env.inherited {
        vars.push(("INHERITED".to_string(), inherited.clone()));
    }

    // Real portage's `PORTAGE_PYM_PATH`: the `lib/` dir of the portage
    // checkout, where the `portage` python package lives. The vendored
    // `bin/` helper scripts that import portage -- `portageq-wrapper`,
    // `ebuild-pyhelper` (and its `chmod-lite`/`doins`/`ebuild-ipc`/…
    // symlinks), `save-ebuild-env.sh` -- each begin with
    // `cd "${PORTAGE_PYM_PATH}" || exit 1`, so leaving it unset makes
    // every one of them abort, which breaks eclass `has_version` /
    // `best_version` (they shell out to `portageq`) for any real ebuild
    // (e.g. `autotools.eclass`'s automake probe). `bin/ebuild.sh`'s own
    // cwd choice still prefers `${PORTAGE_BUILDDIR}/empty` (pre-created
    // by `create_directories`), so setting this does not regress the
    // "safe cwd for bug #469338" branch it was originally left unset for.
    // Only set when the checkout (hence a real `lib/portage`) exists --
    // with no checkout the `.py` helpers can't run regardless.
    let pym_path = portage_checkout().join("lib");
    if pym_path.join("portage").is_dir() {
        vars.push((
            "PORTAGE_PYM_PATH".to_string(),
            pym_path.display().to_string(),
        ));
    }

    vars.extend(extra_env.iter().cloned());
    vars
}

/// `Brush`-backend-only: `phase_env_vars` formatted as real `export
/// NAME=value` bash source text (Rust's own `{:?}` Debug-format
/// double-quoted escaping -- not a full shell-quoting implementation,
/// so a value containing `$`/backtick isn't protected against
/// expansion, but every value here is portuale's own computed path/
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
/// Portuale mirrors that exactly with a fresh `Shell` per phase rather
/// than trying to reuse one across phases (an earlier version of this
/// function did share one shell across a whole command's own prerequisite
/// chain -- confirmed empirically to fail with "cannot mutate readonly
/// variable" on the second phase, exactly the real readonly-variable
/// mechanism working as designed). Real `PORTAGE_BUILDDIR`-relative
/// resume markers (`.pretended`/`.setuped`/`.unpacked`/etc., written by
/// `__dyn_*` themselves) still make a prerequisite phase that's already
/// run cheap to "re-run" from a fresh shell, exactly the way real
/// `doebuild()` itself relies on across its own separate `spawnebuild()`
/// calls -- this isn't a new mechanism invented for portuale.
#[allow(clippy::too_many_arguments)]
async fn run_one_phase(
    env: &Environment,
    root: &Path,
    phase: &str,
    debug: bool,
    extra_env: &[(String, String)],
    config_root: &Path,
    shell: ShellBackend,
    log_file: Option<&Path>,
) -> Result<i32, String> {
    let bin_dir = bin_dir().to_path_buf();
    let helpers_dir = bin_dir.join("ebuild-helpers");

    // `FEATURES={network,ipc,mount,pid}-sandbox` / `FEATURES=sandbox`:
    // the isolated `src_*` phases run as a real subprocess -- wrapped in
    // `unshare` and/or the `sandbox` binary -- regardless of the
    // requested backend. Neither an `unshare(2)` namespace nor an
    // LD_PRELOAD `libsandbox.so` can confine the in-process `Brush`
    // interpreter without taking the whole `portuale` process with it.
    // See this module's own doc comment.
    let iso = phase_isolation(env, phase);
    let effective_shell = if iso.any() { ShellBackend::Bash } else { shell };

    match effective_shell {
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
                log_file,
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
            log_file,
            iso,
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
    log_file: Option<&Path>,
) -> Result<i32, String> {
    let mut shell = brush_core::Shell::builder()
        .default_builtins(brush_builtins::BuiltinSet::BashMode)
        .build()
        .await
        .map_err(|e| format!("brush shell failed to start: {e}"))?;
    let params = brush_phase_params(&mut shell, log_file)?;

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
    log_file: Option<&Path>,
    iso: Isolation,
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
    let mut cmd = sandbox_wrapped_command(&bin_dir.join("ebuild.sh"), phase, iso);
    cmd.envs(vars);
    if let Some(path) = log_file {
        let (out, err) = open_log_file(path)?;
        cmd.stdout(out).stderr(err);
    }
    let status = cmd
        .status()
        .map_err(|e| format!("spawning real bash for phase {phase} failed: {e}"))?;
    Ok(status.code().unwrap_or(1))
}

/// Run one ebuild's `depend` phase (real `EbuildMetadataPhase` /
/// `doebuild(mydo="depend")`) and return the raw metadata keys it emits.
///
/// Real `bin/ebuild.sh`'s `depend` branch (`ebuild.sh:781-804`) writes
/// `KEY=value` lines (`DEPEND RDEPEND SLOT SRC_URI RESTRICT HOMEPAGE
/// LICENSE DESCRIPTION KEYWORDS INHERITED IUSE REQUIRED_USE PDEPEND
/// BDEPEND EAPI PROPERTIES DEFINED_PHASES IDEPEND INHERIT`) to
/// `${PORTAGE_PIPE_FD}` -- real portage's `_metadata_fd` -- so incidental
/// stdout/stderr can't corrupt the metadata. Portuale wires that fd to
/// a `${T}` temp file via a tiny `exec 9>` shell wrapper (no `unsafe`,
/// no extra crate), then parses it back. `depend` is never sandboxed
/// (real `_doebuild_spawn`'s `SANDBOXED_SRC_PHASES` excludes it), so this
/// is a plain `bash bin/ebuild.sh depend` -- no `sandbox`/`unshare`
/// wrapper. Only the `Bash` backend is used (the metadata pipe is a raw
/// fd the in-process `Brush` interpreter can't be handed).
pub(crate) fn run_depend_phase(
    env: &Environment,
    root: &Path,
    config_root: &Path,
    debug: bool,
) -> Result<std::collections::HashMap<String, String>, String> {
    let bin_dir = bin_dir().to_path_buf();
    let helpers_dir = bin_dir.join("ebuild-helpers");
    create_directories(env)?;

    let meta_path = env.t().join(".depend-metadata");
    let _ = std::fs::remove_file(&meta_path);

    let mut vars = phase_env_vars(
        env,
        root,
        "depend",
        debug,
        &bin_dir,
        &helpers_dir,
        config_root,
        &[],
    );
    vars.push(("PORTAGE_PIPE_FD".to_string(), "9".to_string()));

    let mut cmd = std::process::Command::new("bash");
    cmd.arg("-c")
        .arg(r#"exec 9>"$1"; shift; exec "$@""#)
        .arg("portuale-regen") // $0
        .arg(&meta_path) // $1
        .arg("bash")
        .arg(bin_dir.join("ebuild.sh"))
        .arg("depend");
    cmd.envs(vars);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    let out = cmd
        .output()
        .map_err(|e| format!("spawning bash for the depend phase failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr
            .lines()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "depend phase failed for {}/{} (exit {}):\n{tail}",
            env.category,
            env.split.pf,
            out.status.code().unwrap_or(-1),
        ));
    }

    let text =
        std::fs::read_to_string(&meta_path).map_err(|e| format!("{}: {e}", meta_path.display()))?;
    let _ = std::fs::remove_file(&meta_path);
    let mut md = std::collections::HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            md.insert(k.to_string(), v.to_string());
        }
    }
    Ok(md)
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
/// same environment/ebuild-sourcing portuale's own `run_one_phase`
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
    log_file: Option<&Path>,
) -> Result<i32, String> {
    let bin_dir = bin_dir().to_path_buf();
    let helpers_dir = bin_dir.join("ebuild-helpers");

    // Real `_emerge.MiscFunctionsProcess`: `bin/misc-functions.sh` runs
    // `sandbox`-wrapped by default (`free = False` unless
    // `ld_preload_sandbox` says otherwise) -- but with its *own*
    // `SANDBOX_LOG` (`sandbox-misc.log`) so a QA-check violation doesn't
    // clobber the real phase's log. No `unshare` (real
    // `_PostPhaseCommands` passes only `ld_preload_sandbox`, never
    // `networked`). This forces the `Bash` backend, same as a
    // `sandbox`-wrapped phase.
    let fs_sandbox = fs_sandbox_requested() && sandbox_binary().is_some();
    let effective_shell = if fs_sandbox {
        ShellBackend::Bash
    } else {
        shell
    };

    match effective_shell {
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
                log_file,
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
            log_file,
            fs_sandbox,
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
    log_file: Option<&Path>,
) -> Result<i32, String> {
    let mut shell = brush_core::Shell::builder()
        .default_builtins(brush_builtins::BuiltinSet::BashMode)
        .build()
        .await
        .map_err(|e| format!("brush shell failed to start: {e}"))?;
    let params = brush_phase_params(&mut shell, log_file)?;

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
    log_file: Option<&Path>,
    fs_sandbox: bool,
) -> Result<i32, String> {
    let mut vars = phase_env_vars(
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    );
    if fs_sandbox {
        // Real `MiscFunctionsProcess._spawn`: swap in a separate log so
        // a misc-functions violation doesn't clobber the phase's own
        // `sandbox.log`; enable the sandbox (`bin/misc-functions.sh`
        // reads `SANDBOX_DISABLED` the same way `bin/ebuild.sh` does).
        for (k, v) in vars.iter_mut() {
            match k.as_str() {
                "SANDBOX_LOG" => *v = env.t().join("sandbox-misc.log").display().to_string(),
                "SANDBOX_DISABLED" => *v = "0".to_string(),
                _ => {}
            }
        }
    }
    let iso = Isolation {
        fs_sandbox,
        ..Isolation::default()
    };
    let mut cmd = sandbox_wrapped_command(&bin_dir.join("misc-functions.sh"), dyn_command, iso);
    cmd.envs(vars);
    if let Some(path) = log_file {
        let (out, err) = open_log_file(path)?;
        cmd.stdout(out).stderr(err);
    }
    let status = cmd
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
            None,
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
    log_file: Option<&Path>,
    // Caller-supplied env, appended after portuale's own base vars so it
    // overrides them -- the `emerge <atom>` build/merge path passes the
    // resolved `USE` flags here (`bin/ebuild.sh`'s own `use()` reads the
    // `USE` var), which `phase_env_vars` otherwise leaves `""`. `&[]` for
    // a standalone `ebuild <file> <phase>` (no resolved graph entry).
    build_env: &[(String, String)],
) -> Result<i32, String> {
    let env = compute_environment(ebuild_path, portage_tmpdir)?;
    create_directories(&env)?;

    let chain: Vec<&str> = commands
        .iter()
        .flat_map(|&c| phase_prerequisites(c))
        .collect();
    let mut extra_env = vec![("DISTDIR".to_string(), distdir.display().to_string())];
    extra_env.extend(build_env.iter().cloned());
    if chain.contains(&"unpack") {
        let (a, aa) = fetch_sources(&env, root, distdir, debug, config_root, shell).await?;
        extra_env.push(("A".to_string(), a.join(" ")));
        extra_env.push(("AA".to_string(), aa.join(" ")));
    }

    for &command in commands {
        for phase in phase_prerequisites(command) {
            let status = run_one_phase(
                &env,
                root,
                phase,
                debug,
                &extra_env,
                config_root,
                shell,
                log_file,
            )
            .await?;
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
                    log_file,
                )
                .await?;
                if qa_status != 0 {
                    return Ok(qa_status);
                }
                // Real `doebuild(mydo="install")` -> `_post_src_install_
                // write_metadata` (see its own doc comment): the
                // dependency/LICENSE/PROPERTIES/RESTRICT/IUSE build-info
                // files `bin/phase-functions.sh` doesn't write. Run in
                // the same spot -- after `install` + its post-phase
                // misc-functions, before the vdb merge / xpak build reads
                // `build-info`.
                write_post_install_metadata(&env, root, &build_phase_use(build_env))?;
            }
        }
    }
    Ok(0)
}

/// Synchronous entry point for `ebuild.rs` (which is not itself async --
/// `emerge`'s own dispatch never needs an async runtime at all, so this
/// portuale doesn't pay for one there; only this one code path does). Spins
/// up a tokio runtime for the duration of the call -- MUST be
/// multi-threaded (`new_multi_thread`, not `new_current_thread`):
/// confirmed empirically (a single-threaded runtime deadlocks partway
/// through a real multi-phase run -- brush-core's own `Cargo.toml`
/// requires tokio's `rt-multi-thread` feature under unix, not just
/// `rt`, which is the same thing portuale rediscovered the hard way).
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
    build_env: &[(String, String)],
) -> Result<i32, String> {
    run_commands_logged(
        ebuild_path,
        commands,
        root,
        portage_tmpdir,
        distdir,
        debug,
        config_root,
        shell,
        None,
        build_env,
    )
}

/// Like `run_commands`, but when `log_file` is `Some`, every phase (and
/// its post-phase `misc-functions.sh`) has its stdout+stderr captured to
/// that file (append) instead of the terminal -- real portage's
/// `PORTAGE_LOG_FILE` (default `${T}/build.log`). `run_build_scheduler`
/// passes it so a parallel `--jobs` build's output doesn't interleave;
/// the scheduler dumps the file on a build failure.
#[allow(clippy::too_many_arguments)]
pub fn run_commands_logged(
    ebuild_path: &Path,
    commands: &[&str],
    root: &Path,
    portage_tmpdir: &Path,
    distdir: &Path,
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
    log_file: Option<&Path>,
    build_env: &[(String, String)],
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
        log_file,
        build_env,
    ))
}

/// Opens `log_file` for append (creating it and its parent dir), returning
/// two independent handles -- one for a subprocess's stdout, one for its
/// stderr. See `run_commands_logged`.
fn open_log_file(log_file: &Path) -> Result<(std::fs::File, std::fs::File), String> {
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)
        .map_err(|e| format!("{}: {e}", log_file.display()))?;
    let g = f
        .try_clone()
        .map_err(|e| format!("{}: {e}", log_file.display()))?;
    Ok((f, g))
}

/// Builds the `ExecutionParameters` for a brush phase shell, redirecting
/// its stdout+stderr to `log_file` when given (set on the shell's
/// persistent open files so brush's own diagnostics are captured too).
/// The scheduler's *captured* parallel builds run through real `bash`
/// instead (`emerge_build::build_one_source_entry`), where the OS-level
/// fd redirect is complete; this brush path is only reached for a
/// captured build explicitly forced onto the brush backend.
fn brush_phase_params(
    shell: &mut brush_core::Shell,
    log_file: Option<&Path>,
) -> Result<brush_core::ExecutionParameters, String> {
    if let Some(path) = log_file {
        let (out, err) = open_log_file(path)?;
        shell
            .open_files_mut()
            .set_fd(brush_core::openfiles::OpenFiles::STDOUT_FD, out.into());
        shell
            .open_files_mut()
            .set_fd(brush_core::openfiles::OpenFiles::STDERR_FD, err.into());
    }
    Ok(shell.default_exec_params())
}

/// Runs exactly `phase`, with no `actionmap_deps` prerequisite chain --
/// unlike `run_commands`, for phases real portage itself never reaches
/// via `doebuild()`'s own chain at all. Real `dblink.treewalk()` invokes
/// `pkg_preinst`/`pkg_postinst` directly (`EbuildPhase(phase="preinst"/
/// "postinst")`, `lib/portage/dbapi/vartree.py`), not through
/// `doebuild(mydo=...)` -- `ebuild_merge::run_merge` is portuale's own
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
    // See `run_commands_async`'s `build_env` doc: the resolved `USE` for
    // an `emerge <atom>` merge's own `pkg_preinst`/`pkg_postinst`. `&[]`
    // for a standalone phase / a removal hook / a binary merge.
    build_env: &[(String, String)],
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
        run_one_phase(
            &env,
            root,
            phase,
            debug,
            build_env,
            config_root,
            shell,
            None,
        )
        .await
    })
}

/// Like `run_single_phase`, but first seeds `${T}/environment` from a
/// binary package's saved `environment.bz2` so the phase runs against
/// the package's own build-time bash environment (every phase function,
/// eclass-inherited ones included, and the recorded metadata) rather
/// than a re-sourced ebuild -- the only way a binary package's
/// `pkg_preinst`/`pkg_postinst`/`pkg_prerm`/`pkg_postrm` can run at all.
///
/// Real `_emerge/BinpkgEnvExtractor`: `${PORTAGE_BUNZIP2_COMMAND:-
/// ${PORTAGE_BZIP2_COMMAND} -d} -c -- <environment.bz2> > ${T}/environment`,
/// then `touch ${T}/environment.raw` -- the marker real
/// `bin/phase-functions.sh::__preprocess_ebuild_env` checks (its own
/// `[[ -f ${T}/environment.raw ]] || return 0`) before filtering stale
/// `SANDBOX_*`/`FEATURES`/locale vars a different build host may have
/// baked in. `bin/ebuild.sh`'s own top-level code (line ~565) then
/// sources the result and, because `${T}/environment` now exists,
/// skips re-sourcing the ebuild file (line ~617) -- exactly the path a
/// multi-phase source build already exercises between its own phases,
/// so this is not new phase-execution machinery, only a different way
/// of populating `${T}/environment`. Build-time-only path vars (`D`,
/// `ROOT`, `T`, `WORKDIR`, `PORTAGE_BUILDDIR`, `EBUILD`, ...) are never
/// in the saved env -- real `save-ebuild-env.sh` + `__filter_readonly_
/// variables` strip every `portage_readonly_vars` entry when it is
/// written -- so `phase_setup_script`'s own fresh exports win.
///
/// `EMERGE_FROM=binary` (real `doebuild.py:1293` for a binpkg): this
/// selects `__filter_readonly_variables`' own binary branch (filter the
/// untrusted `CATEGORY PVR PF PN PR PV P` from the saved env, so they
/// come from the current, possibly-renamed cpv) and -- load-bearing for
/// `pkg_setup` -- makes `bin/ebuild.sh:616`'s `[[ setup && EMERGE_FROM
/// == ebuild ]]` false, so a binpkg's `pkg_setup` runs from the saved
/// env too instead of re-sourcing (and re-`inherit`-ing, which would
/// `die` -- no repo) the extracted ebuild.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_phase_from_saved_env(
    ebuild_path: &Path,
    saved_env_bz2: &Path,
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

        let dest_env = env.t().join("environment");
        let out =
            std::fs::File::create(&dest_env).map_err(|e| format!("{}: {e}", dest_env.display()))?;
        let status = std::process::Command::new("bzip2")
            .args(["-d", "-c", "--"])
            .arg(saved_env_bz2)
            .stdout(out)
            .status()
            .map_err(|e| format!("failed to spawn bzip2: {e}"))?;
        if !status.success() {
            let _ = std::fs::remove_file(&dest_env);
            return Err(format!(
                "bzip2 failed to decompress {} ({status})",
                saved_env_bz2.display()
            ));
        }
        std::fs::write(env.t().join("environment.raw"), [])
            .map_err(|e| format!("{}: {e}", env.t().join("environment.raw").display()))?;

        let extra_env = [("EMERGE_FROM".to_string(), "binary".to_string())];
        run_one_phase(
            &env,
            root,
            phase,
            debug,
            &extra_env,
            config_root,
            shell,
            None,
        )
        .await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_slot_operator_binds_a_matched_equals_dep_and_leaves_the_rest_alone() {
        let root = std::env::temp_dir().join(format!(
            "portuale-slotbind-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let vdb = root.join("var/db/pkg/dev-libs/foo-1.2");
        std::fs::create_dir_all(&vdb).unwrap();
        std::fs::write(vdb.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(vdb.join("SLOT"), "4/9\n").unwrap();

        // A bare `:=` against an installed dep -> bound to its slot/sub-slot.
        assert_eq!(
            bind_slot_operator("dev-libs/foo:=", &root),
            "dev-libs/foo:4/9="
        );
        // An operator + version + `:=` -> only the slot dep is rewritten.
        assert_eq!(
            bind_slot_operator(">=dev-libs/foo-1:=", &root),
            ">=dev-libs/foo-1:4/9="
        );
        // An already-slotted `:2=` still rebinds (real `vardb.match` +
        // `with_slot`), but a slot the vdb doesn't have -> no match, bare.
        assert_eq!(
            bind_slot_operator("dev-libs/foo:4=", &root),
            "dev-libs/foo:4/9="
        );
        assert_eq!(
            bind_slot_operator("dev-libs/foo:7=", &root),
            "dev-libs/foo:7="
        );
        // Not a `:=` operator, and a `:=` dep with nothing installed:
        // both untouched.
        assert_eq!(
            bind_slot_operator("dev-libs/foo:4", &root),
            "dev-libs/foo:4"
        );
        assert_eq!(
            bind_slot_operator("dev-libs/bar:=", &root),
            "dev-libs/bar:="
        );
        // Not an atom at all (a `||`-group token).
        assert_eq!(bind_slot_operator("||", &root), "||");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn restrict_mirror_from_restrict_evaluates_conditionals_against_the_empty_use_set() {
        assert!(restrict_mirror_from_restrict("mirror"));
        assert!(restrict_mirror_from_restrict("fetch mirror"));
        // deprecated negative spelling still counts (real fetch.py:880)
        assert!(restrict_mirror_from_restrict("nomirror"));
        // no mirror restriction
        assert!(!restrict_mirror_from_restrict(""));
        assert!(!restrict_mirror_from_restrict("fetch strip"));
        // USE-conditional: the fetch-side USE set is always empty here,
        // so `foo? ( mirror )` drops entirely -- not a literal token match
        assert!(!restrict_mirror_from_restrict("foo? ( mirror )"));
        assert!(restrict_mirror_from_restrict("mirror foo? ( strip )"));
    }

    #[test]
    fn restrict_fetch_from_restrict_matches_the_fetch_token_only() {
        assert!(restrict_fetch_from_restrict("fetch"));
        assert!(restrict_fetch_from_restrict("mirror fetch"));
        assert!(!restrict_fetch_from_restrict(""));
        assert!(!restrict_fetch_from_restrict("mirror strip"));
        // USE-conditional drops against the always-empty fetch-side USE
        assert!(!restrict_fetch_from_restrict("foo? ( fetch )"));
    }

    #[test]
    fn network_sandbox_exempt_matches_real_doebuild_spawns_own_formula() {
        // PROPERTIES=live only exempts the unpack phase.
        assert!(network_sandbox_exempt("unpack", "", "live"));
        assert!(!network_sandbox_exempt("compile", "", "live"));
        assert!(!network_sandbox_exempt("test", "", "live"));

        // PROPERTIES=test_network only exempts the test phase.
        assert!(network_sandbox_exempt("test", "", "test_network"));
        assert!(!network_sandbox_exempt("unpack", "", "test_network"));

        // RESTRICT=network-sandbox exempts every phase.
        assert!(network_sandbox_exempt("unpack", "network-sandbox", ""));
        assert!(network_sandbox_exempt("compile", "network-sandbox", ""));
        assert!(network_sandbox_exempt("test", "network-sandbox", ""));

        // None of the three -> not exempt.
        assert!(!network_sandbox_exempt("unpack", "", ""));
        assert!(!network_sandbox_exempt("test", "", ""));

        // USE-conditional groups already dropped by the time these
        // strings arrive (restrict_and_properties's own job) -- a raw
        // conditional token string is simply never matched.
        assert!(!network_sandbox_exempt("unpack", "", "foo? ( live )"));
    }

    #[test]
    fn restrict_and_properties_reads_and_use_reduces_the_real_md5_cache_entry() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-restrict_and_properties_reads_real_md5_cache",
            std::process::id()
        ));
        let env = compute_environment(
            &repo_root.join("dev-libs/propertiespkg/propertiespkg-1.0.ebuild"),
            &portage_tmpdir,
        )
        .expect("real fixture parses");
        let (restrict, properties) = restrict_and_properties(&env);
        assert_eq!(restrict, "");
        assert_eq!(properties, "live");
    }

    #[test]
    fn restrict_and_properties_is_empty_outside_any_repo_checkout() {
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-restrict_and_properties_outside_repo",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("standalone");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        let ebuild = pkg_dir.join("standalone-1.0.ebuild");
        std::fs::write(&ebuild, "EAPI=8\nSLOT=\"0\"\n").unwrap();
        let env = compute_environment(&ebuild, &tmp).expect("standalone ebuild parses");
        assert_eq!(
            restrict_and_properties(&env),
            (String::new(), String::new())
        );
    }

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
    /// ebuild (`fixtures/repo/dev-libs/phasepkg`, whose own
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
            &[],
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
            &[],
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
            &[],
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
            &[],
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
            &[],
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
            &[],
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
            &[],
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
    /// (portuale's own always-empty USE set) but still appear in
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
            &[],
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
    /// `inherit()` function -- previously portuale never populated
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
            &[],
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
    /// fork, see docs/what-this-proves.md's eclass section for the full writeup):
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
                &[],
            );
            let _ = tx.send(result);
        });
        // A generous deadline: this drives a full brush phase chain (many
        // subprocesses), so under a heavily parallel `cargo test` it can
        // legitimately take tens of seconds -- a real pipe-buffer
        // *deadlock* would still hang indefinitely and be caught.
        let status = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("run_commands should complete within the deadline, not deadlock")
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

    /// `compute_environment` reads `INHERITED` from the ebuild's own
    /// `metadata/md5-cache` entry (real `porttree.py:872`) so
    /// `phase_env_vars` can export it and `bin/ebuild.sh` can snapshot it
    /// into `__INHERITED_QA_CACHE` -- suppressing the spurious
    /// `Eclass '…' inherited illegally` QA notice on a phase re-source.
    /// `dev-libs/eclasspkg`'s fixture cache carries `INHERITED=pilotcheck`.
    #[test]
    fn compute_environment_reads_inherited_from_the_md5_cache() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/eclasspkg/eclasspkg-1.0.ebuild");
        let env = compute_environment(&ebuild_path, Path::new("/var/tmp/portage")).unwrap();
        assert_eq!(env.inherited.as_deref(), Some("pilotcheck"));

        // A standalone ebuild outside any repo -> no md5-cache -> None.
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-inherited-none",
            std::process::id()
        ));
        let pkg_dir = tmp.join("dev-libs/standalone");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        let solo = pkg_dir.join("standalone-1.0.ebuild");
        std::fs::write(&solo, "EAPI=8\nSLOT=0\n").unwrap();
        let env = compute_environment(&solo, Path::new("/var/tmp/portage")).unwrap();
        assert_eq!(env.inherited, None);
        let _ = std::fs::remove_dir_all(&tmp);
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
            &[],
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
    /// that guard itself is real, unmodified bash portuale doesn't
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
                &[],
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
            &[],
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0);

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }
}
