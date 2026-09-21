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
// feature under unix, not just `rt`. One process-wide runtime
// (`shared_runtime`), not one per phase -- real `_emerge/Scheduler.py`
// itself runs a whole `emerge` invocation on a single `asyncio` event
// loop, and every one of this module's own sync entry points used to
// pay a fresh thread-pool setup/teardown cost per phase per package
// instead.
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
//       up` plus the real `10.0.0.1/8` + `fd::1/8`
//       `AI_ADDRCONFIG`-workaround addresses (bug #690758) inside (real
//       `_configure_loopback_interface`). `FEATURES=ipc-sandbox` ->
//       `unshare --ipc`.
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
//       specifically to drop privileges *from* an already-root process
//       (a build should not itself run as root); portuale's merge code
//       does now reproduce real's own `os.lchown`/`os.chown` calls
//       (`ebuild_merge.rs`'s `lchown_or_chown`), but that's a *merge*
//       (into `${ROOT}`) concern, not a *build* (`src_compile` etc.,
//       still never run as root) one -- there's still no privilege to
//       drop at build time and nothing for either feature to do here;
//       and, unlike real portage (which only unshares when
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
//   - `PORTAGE_TMPDIR` defaults to `/var/tmp` (real portage's own
//     `make.globals` default; the builddir adds `portage/<cat>/<pf>`
//     itself) but is overridable via the `PORTAGE_TMPDIR`
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
use std::os::unix::io::FromRawFd as _;
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
        .and_then(|repo_root| {
            portage_repo::repo_aux_metadata(&repo_root, &category, &split.pf).ok()
        })
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

    // Real `prepare_build_dirs._prepare_fake_filesdir`
    // (`lib/portage/package/ebuild/prepare_build_dirs.py:504-515`):
    // `${PORTAGE_BUILDDIR}/files` is always a symlink to the ebuild's own
    // `O/files` (the repo package dir's `files/`), even when that target
    // does not exist -- real links unconditionally (a dangling link is
    // fine: `FILESDIR` consumers fail loudly on a missing path either
    // way, and crucially the `clean` phase's `rm -f .../files` only
    // works on a link). The phase env's `FILESDIR` is
    // `Environment::filesdir()` = builddir/files (matching real
    // `doebuild.py:527`), so without this link every `eapply`/
    // `FILESDIR` reference in a real ebuild (app-misc/jq's
    // `jq-1.6-r3-never-bundle-oniguruma.patch` is what surfaced it) dies
    // on a missing path -- L2 S5 finding `l2-filesdir-symlink-missing`.
    // A binary merge's ebuild is copied *into* the builddir (pkg_dir ==
    // portage_builddir), where the link would be a self-loop.
    //
    // Backlog #94 (host `emerge -1 sys-libs/timezone-data` residue):
    // portuale used to link only when the repo `files/` was a real dir
    // and otherwise let `create_directories` make a plain directory --
    // whose `rm -f` in `__dyn_clean` then fails with "Is a directory"
    // and, under the phase's `set -e`, aborts the rest of the cleanup,
    // leaving `.../files` (+ parents) behind in `/var/tmp/portage`.
    // Link unconditionally now; `create_directories` skips the symlink
    // (it must not `create_dir_all` through a dangling link).
    let real_filesdir = pkg_dir.join("files");
    if pkg_dir != portage_builddir {
        ensure_fake_filesdir_link(&real_filesdir, &portage_builddir.join("files"));
    }

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
    /// Real `${T}` -- also read by `ebuild_merge::process_merge_elog`
    /// (the per-package `elog` flush before the post-merge clean, #42).
    pub(crate) fn t(&self) -> PathBuf {
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
/// Real `_prepare_fake_filesdir` (`prepare_build_dirs.py:504-515`) as a
/// standalone step: `link_path` (`${PORTAGE_BUILDDIR}/files`) becomes a
/// symlink to `target` (the repo package dir's `files/`), unconditionally
/// -- dangling included, exactly like real.
///
/// Removal safety (the link target may live anywhere, including the repo
/// itself): `symlink_metadata` never follows the link, so only the link
/// entry (or a plain file) is ever `remove_file`d; a real directory --
/// the pre-fix leftover shape, or anything a phase dropped -- is removed
/// with `remove_dir_all`, but strictly bounded to this one known
/// inside-the-builddir path (never a user path, never followed). The
/// parent builddir is `create_dir_all`ed first so a first-ever build has
/// somewhere to put the link.
fn ensure_fake_filesdir_link(target: &Path, link_path: &Path) {
    if link_path
        .parent()
        .is_some_and(|parent| std::fs::create_dir_all(parent).is_err())
    {
        return;
    }
    match std::fs::read_link(link_path) {
        Ok(current) if current == target => {}
        _ => {
            match std::fs::symlink_metadata(link_path) {
                Ok(meta) if meta.file_type().is_symlink() || meta.file_type().is_file() => {
                    let _ = std::fs::remove_file(link_path);
                }
                Ok(_) => {
                    let _ = std::fs::remove_dir_all(link_path);
                }
                Err(_) => {}
            }
            let _ = std::os::unix::fs::symlink(target, link_path);
        }
    }
}

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
        // `${PORTAGE_BUILDDIR}/files` is a symlink (possibly dangling --
        // see `ensure_fake_filesdir_link`), never a directory to create:
        // `create_dir_all` would follow a dangling link and fail (or
        // worse, materialise the target). `symlink_metadata` does not
        // follow. Anything else in this list is always a real dir.
        if dir == env.filesdir()
            && std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink())
        {
            continue;
        }
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
    // Backlog #88: the overlay this process created (if any), removed
    // at process exit. `DIR` itself cannot own the cleanup: statics
    // never run destructors, and `bin_dir()` has seven call sites
    // across three files with independent exit paths, so per-site
    // removal would both sprawl and rot. One `atexit` registration,
    // made exactly where the directory is created, covers every
    // command (and the test harness's subprocess runs) on all normal
    // exits; signal kills still leak, like any tmp dir.
    static CREATED: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let vendored = repo_root().join("bin");
        let checkout = portage_checkout().join("bin");
        if !checkout.is_dir() {
            return vendored;
        }
        let overlay = std::env::temp_dir().join(format!("portuale-bin.{}", std::process::id()));
        match build_bin_overlay(&overlay, &checkout, &vendored) {
            Ok(()) => {
                let _ = CREATED.set(overlay.clone());
                // `extern "C"`, no closure capture: the handler reads
                // `CREATED` itself. A failed registration keeps the
                // old leak rather than breaking the run (best effort,
                // like the overlay fallback below it).
                extern "C" fn cleanup_created_bin_overlay() {
                    if let Some(dir) = CREATED.get() {
                        remove_bin_overlay_dir(dir);
                    }
                }
                let _ = unsafe { libc::atexit(cleanup_created_bin_overlay) };
                overlay
            }
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

/// Removes a `bin_dir()` overlay directory, ignoring every failure
/// (a partially-removed or already-gone overlay is not an error worth
/// failing a phase -- or process exit -- over). Called by the `atexit`
/// handler; factored out so the idempotency contract is unit-pinned.
fn remove_bin_overlay_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Populates `overlay` with a symlink to every entry of `checkout`, then
/// symlinks every entry of `vendored` over the top (dropping the
/// checkout link first). Rebuilt from scratch each call.
fn build_bin_overlay(overlay: &Path, checkout: &Path, vendored: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::symlink;
    let _ = std::fs::remove_dir_all(overlay);
    std::fs::create_dir_all(overlay)?;
    for entry in portage_util::read_dir_entries(checkout)? {
        symlink(entry.path(), overlay.join(entry.file_name()))?;
    }
    for entry in portage_util::read_dir_entries(vendored)? {
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

/// Real `RESTRICT=primaryuri` (real `fetch.py:1187`, `"primaryuri" in
/// restrict`), USE-conditional-evaluated the same way
/// `restrict_mirror_from_restrict` does. Moves the file's own literal
/// `SRC_URI` URIs to the front of its candidate list (see
/// `FetchOptions::restrict_primaryuri`).
pub(crate) fn restrict_primaryuri_from_restrict(restrict: &str) -> bool {
    flat_field_has_token(restrict, &["primaryuri"])
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
/// `flat_field_has_token` already uses. `use_set` is the reduction
/// input: empty for every phase but `depend` (see
/// `restrict_and_properties`), the config `USE` set for `depend` (see
/// `depend_use_set`).
///
/// Deduped and sorted like real `config.py::_flatten` (`:1681-1688`):
/// `" ".join(sorted(set(use_reduce(..., flat=True))))`. This is
/// load-bearing, not cosmetic: ncurses' `RESTRICT="!test? ( test ) test"`
/// use-reduces (with `test` off) to two `test` tokens, and `bin/ebuild.sh:
/// 721-724` overwrites the phase's `RESTRICT` with this value -- without
/// the set, the saved env (and the vdb `environment.bz2`) carries
/// `RESTRICT="test test"` where real has `"test"` (#45 P2a finding).
fn flat_field_on(raw: &str, use_set: &std::collections::HashSet<String>) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let tokens: Vec<String> = raw.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat(&tokens, use_set, portage_use_reduce::MatchMode::Normal)
        .map(|flat| {
            let unique: std::collections::BTreeSet<String> = flat.into_iter().collect();
            unique.into_iter().collect::<Vec<_>>().join(" ")
        })
        .unwrap_or_default()
}

/// Real `config.environ()` for a standalone `ebuild <file> <phase>`:
/// the package's effective `USE` plus the whole resolved config env --
/// `phase_environ(config, None)` (make.conf + profile + env layer, with
/// the same `PORTUALE_COMPUTED` exclusions the merge path's run-wide env
/// already carries) -- exported as base vars into every phase. Merge builds pass their fully-resolved flags via `extra_env`
/// (appended after these base vars downstream, so it keeps overriding
/// them), which means this computation only ever surfaces for standalone
/// `ebuild <file> <phase>` runs -- resolved the same way `ebuild_merge::
/// blocked_installed_packages`' own standalone resolution does
/// (`find_repos` + `resolve_config` + md5-cache `IUSE` +
/// `effective_use_flags`, via `candidate_use_flags_display` for USE and
/// `phase_environ` for the flags), and empty on any failure (missing
/// `repos.conf`, unreadable cache, an ebuild path outside any real
/// repo).
///
/// Two gates, both load-bearing. When `extra_env` already carries `USE`
/// the whole computation is skipped (zero cost and zero behavior change
/// for merge builds -- their `extra_env` carries the complete flag set
/// from the same config, so nothing is lost). And the `depend` phase
/// keeps the old empty base exactly: metadata extraction must stay on
/// the empty set -- config-derived values there would rewrite `--regen`
/// cache bytes for USE-conditional metadata and reload the whole profile
/// once per ebuild (against the F.10/F.11 perf work); every pinned
/// golden assumes it.
///
/// Standalone `package.env`: both halves flow here, not just on merge
/// builds. The `USE=` half rides `candidate_use_flags_display` above
/// (`effective_use_flags` atom-matches `package_env_use` itself); the
/// build-vars half is atom-matched below against the ebuild's own
/// md5-cache identity (`match_package_env_vars`).
///
/// `(USE display pairs, build flag pairs)` from one standalone config
/// load -- the alias keeps the loader closure below under clippy's
/// `type_complexity` lint.
type StandaloneBaseEnv = (Vec<(String, bool)>, Vec<(String, String)>);

fn phase_standalone_base_env(
    env: &Environment,
    config_root: &Path,
    eroot: &Path,
    ebuild_phase_value: &str,
    extra_env: &[(String, String)],
) -> (String, Vec<(String, String)>) {
    if ebuild_phase_value == "depend" || extra_env.iter().any(|(k, _)| k == "USE") {
        return (String::new(), Vec::new());
    }
    let Some((display, flags)) = (|| -> Option<StandaloneBaseEnv> {
        // Gate on a real repo checkout (same tolerance as
        // `restrict_and_properties` below): outside one there is no
        // md5-cache to read IUSE from.
        repo_root_for(&env.pkg_dir)?;
        let repos = portage_repo::find_repos(config_root).ok()?;
        let main_repo = repos.iter().find(|r| r.is_main)?;
        let overlay_repos: Vec<(String, PathBuf)> = repos
            .iter()
            .filter(|r| !r.is_main)
            .map(|r| (r.name.clone(), r.location.clone()))
            .collect();
        let repo_aliases: Vec<(String, PathBuf)> = repos
            .iter()
            .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
            .collect();
        let repo_masters: std::collections::HashMap<String, Vec<PathBuf>> = repos
            .iter()
            .map(|r| (r.name.clone(), r.masters.clone()))
            .collect();
        let config = portage_profile::resolve_config(
            config_root,
            &main_repo.location,
            &overlay_repos,
            &repo_aliases,
            &main_repo.name,
            &repo_masters,
            eroot,
        )
        .ok()?;
        let display = portage_repo::candidate_use_flags_display(
            &repos,
            &config,
            &env.category,
            &env.split.pn,
            &env.split.pvr,
        );
        let flags = portage_profile::phase_environ(&config, None);
        // Per-package `package.env` build vars, atom-matched against
        // the ebuild's own md5-cache identity (real `_grab_pkg_env`
        // folding a matching entry into `configdict["pkg"]`): the
        // standalone equivalent of the merge path's
        // `entry_package_env_vars`, which needed a resolved graph
        // entry only to name the same `cat/pkg-ver:slot/sub` string
        // rebuilt here. Layered after the base flags so they win,
        // like the merge path's own `entry_build_env` order. The
        // `USE=` half needs no separate step: `candidate_use_flags_display`
        // above already folds `package_env_use` in via
        // `effective_use_flags`' own atom matching.
        let slot_raw = portage_repo::repo_aux_metadata(
            &repo_root_for(&env.pkg_dir)?,
            &env.category,
            &env.split.pf,
        )
        .ok()
        .and_then(|m| m.get("SLOT").cloned())
        .unwrap_or_default();
        // Same `slot`/`sub_slot` fallback shape as the merge path's
        // `entry_package_env_vars` (missing `SLOT` means slot `0`).
        let (slot, sub_slot) = match slot_raw.split_once('/') {
            Some((s, ss)) => (s.to_string(), ss.to_string()),
            None if slot_raw.is_empty() => ("0".to_string(), "0".to_string()),
            None => (slot_raw.clone(), slot_raw),
        };
        let cpv_slot = format!(
            "{}/{}-{}:{}/{}",
            env.category, env.split.pn, env.split.pvr, slot, sub_slot
        );
        let mut flags = flags;
        // Real's layer stacking: the run-wide base is the full resolved
        // config env here (`phase_environ`), an incremental `package.env`
        // value folds onto it in `[base, pkg, calling-env]` order, and a
        // scalar loses to the calling environment when it carries the
        // same key (#101).
        let profile_only_variables = config
            .resolved_incremental("PROFILE_ONLY_VARIABLES")
            .unwrap_or_default();
        let base = flags.clone();
        flags.extend(match_package_env_vars(
            &config.package_env_vars,
            &cpv_slot,
            &profile_only_variables,
            &base,
            &portage_profile::config_env_all(),
        ));
        Some((display, flags))
    })() else {
        return (String::new(), Vec::new());
    };
    let enabled: Vec<&str> = display
        .iter()
        .filter(|(_, on)| *on)
        .map(|(f, _)| f.as_str())
        .collect();
    (enabled.join(" "), flags)
}

/// Real `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` (`doebuild_environment()`
/// sets both, unconditionally, for every phase): the ebuild's own
/// `RESTRICT`/`PROPERTIES` metadata, USE-reduced. Read from the same
/// repo's own `metadata/md5-cache` entry `fetch_sources`'s own `RESTRICT`
/// read already trusts. `use_set` is the reduction input -- empty for
/// every phase (no resolved graph reaches a standalone `ebuild <file>
/// <phase>`, and `entry_build_env`'s own resolved USE (an `emerge -b`
/// build) doesn't reach this deep yet either -- see this module's own
/// "KNOWN, DOCUMENTED GAPS"), except `depend` (see `depend_use_set`),
/// whose config-`USE` reduction real `doebuild(mydo="depend")` runs
/// with. `("", "")` outside any repo checkout, matching
/// `repo_root_for`'s own established tolerance.
fn restrict_and_properties(
    env: &Environment,
    use_set: &std::collections::HashSet<String>,
) -> (String, String) {
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return (String::new(), String::new());
    };
    let metadata = portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf).ok();
    let get = |key: &str| {
        metadata
            .as_ref()
            .and_then(|m| m.get(key))
            .map(String::as_str)
            .unwrap_or("")
    };
    (
        flat_field_on(get("RESTRICT"), use_set),
        flat_field_on(get("PROPERTIES"), use_set),
    )
}

/// Real `const.INCREMENTALS` (`3rdparty/portage/lib/portage/const.py:125`):
/// a key in this list is appended to the container's current value by
/// `_grab_pkg_env`, then folded onto the lower layers by `regenerate()`
/// (`config.py:2778-2825`: `-*` clears the set, `-tok` removes, sorted
/// union). Kept complete even though the acceptance gate below drops most
/// of them, so a future gate change cannot silently lose the semantics.
const PACKAGE_ENV_INCREMENTALS: [&str; 12] = [
    "ACCEPT_KEYWORDS",
    "CONFIG_PROTECT",
    "CONFIG_PROTECT_MASK",
    "ENV_UNSET",
    "FEATURES",
    "IUSE_IMPLICIT",
    "PROFILE_ONLY_VARIABLES",
    "USE",
    "USE_EXPAND",
    "USE_EXPAND_HIDDEN",
    "USE_EXPAND_IMPLICIT",
    "USE_EXPAND_UNPREFIXED",
];

fn is_package_env_incremental(key: &str) -> bool {
    PACKAGE_ENV_INCREMENTALS.contains(&key)
}

/// Real `_grab_pkg_env`'s per-key acceptance gate
/// (`config.py:2269-2300`), minus the keys portuale's own phase pipeline
/// owns:
///
/// * `USE` is real's special case (popped before the call, re-appended
///   after `package.use`); portuale folds it via
///   `Config::package_env_use`, so it must not also appear here.
/// * `ENV_BLACKLIST` is real's `env_blacklist` (it already covers
///   `PKGUSE`, the only `protected_pkg_keys` member).
/// * `ENVIRON_FILTER` keys real accepts into the config but never exports
///   to a phase (`config.environ()`, `config.py:3263-3350`); portuale's
///   `package_env_vars` layer only exists in the phase env, so they are
///   dropped.
/// * `PORTUALE_COMPUTED` keys portuale's own phase runner computes and
///   must always win (`FEATURES` and `PORTAGE_TMPDIR` are filed residues
///   #98/#99; `PATH`, `DISTDIR`, `D`, … are owned by design).
/// * `profile_only_variables` is the profile's dynamic
///   `PROFILE_ONLY_VARIABLES` (`config.py:724-733`; `ARCH`, `ELIBC`,
///   `IUSE_IMPLICIT`, `USE_EXPAND*`, …).
///
/// Everything else is accepted, including empty values (real blanks a
/// variable with `VAR=""`) and `__`-prefixed names (real does not strip
/// them here).
fn package_env_key_allowed(key: &str, profile_only_variables: &[String]) -> bool {
    !key.is_empty()
        && key != "USE"
        && !portage_profile::ENV_BLACKLIST.contains(&key)
        && !portage_profile::ENVIRON_FILTER.contains(&key)
        && !portage_profile::PORTUALE_COMPUTED.contains(&key)
        && !profile_only_variables.iter().any(|k| k == key)
}

/// Real `regenerate()`'s incremental fold (`config.py:2735`, `:2778-2825`)
/// for one key, in real's layer order `[base-lower, pkg, calling-env]`:
/// the already-folded lower layers' tokens, then the package.env value's
/// tokens in order, then the calling environment's tokens in order
/// (`-*` clears, `-tok` removes), sorted and space-joined. The calling
/// env folds **last**, so a `-tok` there prunes a token a package.env
/// file added (backlog #101, S0 cell B), while a package.env `-tok`
/// cannot prune a calling-env token. Re-applying the calling-env tokens
/// after a base that already folded them in is idempotent (set
/// add/remove), so `base_lower` may be the caller's own already-folded
/// value for the key (`""` when the run-wide env doesn't carry it).
pub(crate) fn fold_package_env_incremental(
    base_lower: &str,
    pkg: &str,
    calling_env: &str,
) -> String {
    let mut set: std::collections::BTreeSet<String> =
        base_lower.split_whitespace().map(String::from).collect();
    for tok in pkg.split_whitespace().chain(calling_env.split_whitespace()) {
        if tok == "-*" {
            set.clear();
        } else if let Some(rest) = tok.strip_prefix('-') {
            set.remove(rest);
        } else {
            set.insert(tok.to_string());
        }
    }
    set.into_iter().collect::<Vec<_>>().join(" ")
}

/// Atom-match `package_env_vars` (real `_grab_pkg_env` folding a
/// matching `/etc/portage/package.env` entry's env file into
/// `configdict["pkg"]`) against one `cat/pkg-ver:slot/sub` string.
///
/// The container follows real exactly: files of one entry in order,
/// matching entries in list order (portuale keeps parse order; real
/// applies `ordered_by_atom_specificity`, a pre-existing narrowing), an
/// incremental appends, a scalar replaces, an empty value blanks.
/// Incrementals are then folded in real's layer order
/// `[base-lower, pkg, calling-env]` (`regenerate()`), and a package.env
/// **scalar is dropped when the calling environment carries the same
/// key**: real's `USE_ORDER` puts the `env` layer above `pkg`
/// (`config.py:1031-1035`), so the process value wins (backlog #101, S0
/// cell A). `calling_env` is the same source `phase_environ`'s step 2
/// uses (`portage_profile::config_env_all()`), compared by key presence
/// — *not* `base_env` as a whole, which also contains config scalars a
/// package.env value legitimately outranks.
///
/// Shared by the merge path (`emerge_build::entry_package_env_vars`,
/// matching a resolved graph entry and passing `options.build_env`) and
/// the standalone path below (matching the ebuild's own md5-cache
/// identity and passing the narrow base -- no resolved graph needed).
/// Both paths thread the real calling environment, so the precedence
/// rule holds on both: the S0 capture shows the standalone path is
/// inverted today too, the same bug in this shared function (S2's base
/// replacement is a separate change and is untouched here).
pub(crate) fn match_package_env_vars(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    profile_only_variables: &[String],
    base_env: &[(String, String)],
    calling_env: &[(String, String)],
) -> Vec<(String, String)> {
    let mut container: Vec<(String, String)> = Vec::new();
    for (atom, vars) in package_env_vars {
        if !portage_dep::match_from_list(atom, &[cpv_slot]).is_some_and(|m| !m.is_empty()) {
            continue;
        }
        for (k, v) in vars {
            if !package_env_key_allowed(k, profile_only_variables) {
                continue;
            }
            if is_package_env_incremental(k) {
                match container.iter_mut().rev().find(|(ck, _)| ck == k) {
                    Some((_, current)) => {
                        if !current.is_empty() && !v.is_empty() {
                            current.push(' ');
                        }
                        current.push_str(v);
                    }
                    None => container.push((k.clone(), v.clone())),
                }
            } else {
                container.retain(|(ck, _)| ck != k);
                container.push((k.clone(), v.clone()));
            }
        }
    }
    container
        .into_iter()
        .filter_map(|(k, v)| {
            if is_package_env_incremental(&k) {
                let base = base_env
                    .iter()
                    .rev()
                    .find(|(bk, _)| *bk == k)
                    .map(|(_, bv)| bv.as_str())
                    .unwrap_or("");
                let calling = calling_env
                    .iter()
                    .rev()
                    .find(|(ck, _)| *ck == k)
                    .map(|(_, cv)| cv.as_str())
                    .unwrap_or("");
                Some((k, fold_package_env_incremental(base, &v, calling)))
            } else if calling_env.iter().any(|(ck, _)| *ck == k) {
                // Real's `env` layer outranks `pkg` for a scalar: the
                // process value (already in the caller's base env) wins,
                // so the package.env value is dropped, not layered.
                None
            } else {
                Some((k, v))
            }
        })
        .collect()
}

/// The config-`USE` set the `depend` phase reduces `RESTRICT`/
/// `PROPERTIES` on: real `doebuild(mydo="depend")` runs with the
/// `setcpv` config `USE` (profile + `make.conf` + user `package.use`,
/// no per-package IUSE resolution -- the phase *generates* IUSE), and
/// `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` come from that same
/// already-USE-reduced `_pkg` accessor (`_flatten` over
/// `settings["PORTAGE_USE"]`). Portuale's equivalent is
/// `Config::use_flags` (the same config-global set, no per-candidate
/// layer). Empty when no repo checkout or config load is available --
/// the pre-existing empty-set behavior, not an error. Atom-matched
/// `package.use` / `package.env` entries for the depend target are NOT
/// folded in (matching needs the target's `cat/pkg-ver:slot/sub`
/// identity, which the `depend` path deliberately doesn't resolve --
/// metadata extraction stays on the config-global set).
fn depend_use_set(
    env: &Environment,
    config_root: &Path,
    eroot: &Path,
) -> std::collections::HashSet<String> {
    let Some(_) = repo_root_for(&env.pkg_dir) else {
        return std::collections::HashSet::new();
    };
    let repos = match portage_repo::find_repos(config_root) {
        Ok(repos) => repos,
        Err(_) => return std::collections::HashSet::new(),
    };
    let Some(main_repo) = repos.iter().find(|r| r.is_main) else {
        return std::collections::HashSet::new();
    };
    let overlay_repos: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let repo_aliases: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    let repo_masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    portage_profile::resolve_config(
        config_root,
        &main_repo.location,
        &overlay_repos,
        &repo_aliases,
        &main_repo.name,
        &repo_masters,
        eroot,
    )
    .map(|config| config.use_flags)
    .unwrap_or_default()
}

/// The resolved config for the distfile fetch path: real `fetch.py` reads
/// `FETCHCOMMAND`/`RESUMECOMMAND` (and the `_<PROTO>` variants) and
/// `PORTAGE_SSH_OPTS` out of the same `mysettings` the phases run with,
/// so portuale resolves the same chain once here (the `depend_use_set`
/// resolution above is kept as a sibling rather than factored, since
/// this needs the whole `Config`, not just `use_flags`).
fn resolved_fetch_config(
    env: &Environment,
    config_root: &Path,
    eroot: &Path,
) -> Option<portage_profile::Config> {
    let _ = repo_root_for(&env.pkg_dir)?;
    let repos = portage_repo::find_repos(config_root).ok()?;
    let main_repo = repos.iter().find(|r| r.is_main)?;
    let overlay_repos: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let repo_aliases: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    let repo_masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    portage_profile::resolve_config(
        config_root,
        &main_repo.location,
        &overlay_repos,
        &repo_aliases,
        &main_repo.name,
        &repo_masters,
        eroot,
    )
    .ok()
}

/// Real `fetch.py:1055-1059`: `shlex.split(PORTAGE_RO_DISTDIRS)` filtered
/// to directories that exist (each candidate is re-checked at fetch
/// time, but real filters the list up front).
fn resolved_ro_distdirs(
    env: &Environment,
    config_root: &Path,
    eroot: &Path,
) -> Vec<std::path::PathBuf> {
    let _ = repo_root_for(&env.pkg_dir);
    let Some(repos) = portage_repo::find_repos(config_root).ok() else {
        return Vec::new();
    };
    let Some(main_repo) = repos.iter().find(|r| r.is_main) else {
        return Vec::new();
    };
    let overlay_repos: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let repo_aliases: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    let repo_masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    portage_profile::resolve_config(
        config_root,
        &main_repo.location,
        &overlay_repos,
        &repo_aliases,
        &main_repo.name,
        &repo_masters,
        eroot,
    )
    .ok()
    .and_then(|config| config.other_vars.get("PORTAGE_RO_DISTDIRS").cloned())
    .map(|value| {
        portage_fetch::split_shell_words(&value)
            .into_iter()
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_dir())
            .collect()
    })
    .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
async fn fetch_sources(
    env: &Environment,
    root: &Path,
    distdir: &Path,
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
    features: &str,
    use_flags: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let metadata = portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf).ok();
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
    let restrict_primaryuri = restrict
        .map(|r| restrict_primaryuri_from_restrict(r))
        .unwrap_or(false);
    // Real `AA`: the keys of `_parse_uri_map` -- each distfile once.
    let mut aa: Vec<String> = Vec::new();
    for entry in portage_fetch::flatten_src_uri(&src_uri, |_, _| true)
        .map_err(|e| format!("{}: {e}", env.pkg_dir.display()))?
    {
        if !aa.contains(&entry.filename) {
            aa.push(entry.filename);
        }
    }
    // Real `fetch.py` reads its commands and `PORTAGE_SSH_OPTS` out of
    // the same settings object the phases use; resolve the chain once.
    let fetch_config = resolved_fetch_config(env, config_root, root);
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
            distlocks: features.split_whitespace().any(|tok| tok == "distlocks"),
            restrict_mirror,
            restrict_fetch,
            restrict_primaryuri,
            // Real `FEATURES=force-mirror` -- same env-var shortcut as
            // `distlocks` above (no full config resolution on this
            // path), defaulting to real `false`.
            force_mirror: features.split_whitespace().any(|tok| tok == "force-mirror"),
            use_flags: use_flags.split_whitespace().map(String::from).collect(),
            // Real `fetch.py:1652-1700`'s command family out of the same
            // settings object the phases use.
            fetch_commands: fetch_config.as_ref().map(fetch::fetch_commands_from_config),
            // Real `mysettings.get("PORTAGE_SSH_OPTS")` (`fetch.py:1806`).
            portage_ssh_opts: fetch_config
                .as_ref()
                .and_then(fetch::portage_ssh_opts_from_config),
            // Real `fetch.py:1055-1059`: existing directories only.
            ro_distdirs: resolved_ro_distdirs(env, config_root, root),
            mirror_cache_now: None,
            // Real `PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS` -- same env-var
            // shortcut as `distlocks` above.
            checksum_failure_max_tries: {
                let value = std::env::var("PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS").ok();
                let (tries, warnings) = fetch::checksum_failure_max_tries(value.as_deref());
                for warning in warnings {
                    eprintln!("{warning}");
                }
                tries
            },
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
    build_env: &[(String, String)],
) -> Result<(), String> {
    let build_info = env.build_info();

    // Real `_post_src_install_write_metadata` (`doebuild.py:2727-2732`)
    // writes `int(time.time())` into `build-info/BUILD_TIME` before any
    // metadata key, unconditionally (backlog #47). The vdb entry copies
    // it (`write_vdb_entry_from_dir`) and `_consolidate_to_metadata_file`
    // folds it into the consolidated `metadata` body -- real's own
    // reader (`versions.py:413`) and binpkg multi-instance logic compare
    // against it, and omitting it made every portuale vdb entry differ
    // from real's by the `BUILD_TIME` file *and* the `metadata` field.
    let build_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("system clock before epoch: {e}"))?
        .as_secs();
    std::fs::write(build_info.join("BUILD_TIME"), format!("{build_time}\n"))
        .map_err(|e| format!("{}: {e}", build_info.join("BUILD_TIME").display()))?;

    let use_flags = build_phase_use(build_env);
    let iuse_effective = build_env
        .iter()
        .find(|(k, _)| k == "IUSE_EFFECTIVE")
        .map(|(_, v)| v.trim())
        .unwrap_or("");
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return Ok(());
    };
    let Ok(metadata) = portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf)
    else {
        return Ok(());
    };

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
            &use_flags,
            portage_use_reduce::MatchMode::Normal,
        )
        .map_err(|e| format!("{}: build-info/{key}: {e}", env.pkg_dir.display()))?;
        // Real `_post_src_install_write_metadata` with `token_class=Atom`
        // (`doebuild.py:2749`): every dependency-atom token's own USE
        // deps are evaluated against this package's effective USE before
        // the result is stored -- `Atom.evaluate_conditionals`,
        // `lib/portage/dep/__init__.py:1387`; an unknown/disabled
        // `flag?` use-dep is *dropped* (bug #386829's canonicalisation),
        // while a `flag?` group conditional was already handled by
        // `use_reduce_structured` above. `evaluate_atom_conditionals` is
        // the port; a token that isn't an atom (a `||` marker, a bare
        // paren) passes through unchanged.
        //
        // Then the `*DEPEND` keys go through real
        // `evaluate_slot_operator_equal_deps`: bind every `:=` atom to
        // the installed dependency's `<slot>/<sub-slot>=`.
        let value = if key.ends_with("DEPEND") {
            reduced
                .into_iter()
                .map(|tok| {
                    let tok =
                        portage_dep::evaluate_atom_conditionals(&tok, &use_flags).unwrap_or(tok);
                    bind_slot_operator(&tok, root)
                })
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

    // Real `_post_src_install_write_metadata` (`doebuild.py:2720-2744`):
    // `IUSE` is written verbatim from the package config ("in case it's
    // corrupted due to local environment settings", bug #386829) and is
    // *always* present -- a no-IUSE ebuild gets an empty
    // `metadata/IUSE`, which is exactly what real archives carry.
    // `IUSE_EFFECTIVE` (EAPI 5+) is the profile's own
    // `_calc_iuse_effective` result, threaded here as an ordinary phase
    // env key by #37 S1 (`portage_profile::phase_environ`); absent for a
    // standalone `ebuild <file>` run with no resolved config, in which
    // case real's `configdict` has no such key either.
    let iuse = metadata.get("IUSE").map(|s| s.trim()).unwrap_or("");
    std::fs::write(build_info.join("IUSE"), format!("{iuse}\n"))
        .map_err(|e| format!("{}: {e}", build_info.join("IUSE").display()))?;
    if !iuse_effective.is_empty() {
        std::fs::write(
            build_info.join("IUSE_EFFECTIVE"),
            format!("{iuse_effective}\n"),
        )
        .map_err(|e| format!("{}: {e}", build_info.join("IUSE_EFFECTIVE").display()))?;
    }

    // Real `_post_src_install_uid_fix` (`doebuild.py:2997-3004`): the
    // package's own installed size, summed over `${D}` -- regular files
    // only, and hardlinked inodes counted exactly once (the same
    // `counted_inodes` dedup real uses). Written into `build-info/SIZE`,
    // which both the archive metadata (`metadata/SIZE`, #39) and the vdb
    // copy.
    let size = dir_size_bytes(&env.d())?;
    std::fs::write(build_info.join("SIZE"), format!("{size}\n"))
        .map_err(|e| format!("{}: {e}", build_info.join("SIZE").display()))?;
    Ok(())
}

/// Real `_post_src_install_soname_symlinks` (`doebuild.py:3069-3300`),
/// run where real runs it: after the `install_qa_check
/// install_symlink_html_docs install_hooks` misc-function sequence, in
/// `_emerge/EbuildPhase._commands_exit`. Four things, all #39:
///
/// 1. Rewrite `build-info/NEEDED.ELF.2` with the 6th, trailing multilib
///    category field -- real reads each object's own ELF header
///    (`ELFHeader.read`) and writes `NeededEntry.__str__`'s
///    `arch;obj;soname;rpaths;needed;category` line back. A line whose
///    object no longer reads as ELF keeps an empty category (real's
///    `multilib_category = None`).
/// 2. Generate `build-info/REQUIRES`/`PROVIDES` from the recognized
///    entries (`SonameDepsProcessor`, `PROVIDES_EXCLUDE`/
///    `REQUIRES_EXCLUDE` aware); real writes each file only when the
///    corresponding map is non-empty.
/// 3. Report a `QA Notice: Missing soname symlink(s):` block when a
///    library's own soname symlink is missing (real only *reports*;
///    it never creates the link here).
/// 4. Append `_inject_libc_dep`'s implicit `>=<installed libc>` to
///    `build-info/RDEPEND` (bug #753500). Real reaches this function
///    only when `NEEDED.ELF.2` exists, so a package with no ELF gets
///    neither `PROVIDES`/`REQUIRES` nor the libc dep -- matched by the
///    early return below.
fn write_post_install_soname_deps(env: &Environment, root: &Path) -> Result<(), String> {
    let build_info = env.build_info();
    let needed_path = build_info.join("NEEDED.ELF.2");
    let Ok(text) = std::fs::read_to_string(&needed_path) else {
        return Ok(());
    };

    let mut rewritten = String::new();
    let mut recognized: Vec<crate::needed_elf::NeededEntry> = Vec::new();
    let mut missing_symlinks: Vec<(String, String)> = Vec::new();
    let libpaths =
        crate::needed_elf::getlibpaths(root, std::env::var("LD_LIBRARY_PATH").ok().as_deref());
    for mut entry in crate::needed_elf::NeededEntry::parse_file(&text) {
        let obj_path = env.d().join(entry.filename.trim_start_matches('/'));
        if let Some(cat) = crate::needed_elf::compute_multilib_category(&obj_path) {
            entry.multilib_category = Some(cat);
            recognized.push(entry.clone());
        }
        // Real's own symlink QA: only for an entry with an soname in a
        // real libdir, and never created -- reported only.
        if !entry.soname.is_empty() {
            let obj_dir = obj_path.parent().map(|p| p.to_path_buf());
            let parent_rel = std::path::Path::new(&entry.filename)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let in_libdir = libpaths
                .iter()
                .any(|l| l.trim_end_matches('/') == parent_rel.trim_end_matches('/'));
            if in_libdir
                && let Some(dir) = obj_dir
                && !dir.join(&entry.soname).exists()
            {
                missing_symlinks.push((entry.filename.clone(), entry.soname.clone()));
            }
        }
        rewritten.push_str(&entry.to_needed_line());
    }
    std::fs::write(&needed_path, rewritten)
        .map_err(|e| format!("{}: {e}", needed_path.display()))?;

    let read_trim = |name: &str| {
        std::fs::read_to_string(build_info.join(name))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let provides_exclude = read_trim("PROVIDES_EXCLUDE");
    let requires_exclude = read_trim("REQUIRES_EXCLUDE");
    let (provides, requires) =
        crate::needed_elf::generate_soname_deps(&recognized, &provides_exclude, &requires_exclude);
    if let Some(requires) = requires {
        let path = build_info.join("REQUIRES");
        std::fs::write(&path, requires).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    if let Some(provides) = provides {
        let path = build_info.join("PROVIDES");
        std::fs::write(&path, provides).map_err(|e| format!("{}: {e}", path.display()))?;
    }

    if !missing_symlinks.is_empty() {
        eprintln!("QA Notice: Missing soname symlink(s):");
        eprintln!();
        for (obj, soname) in &missing_symlinks {
            let dir = std::path::Path::new(obj)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            eprintln!(
                "\t{} -> {}",
                std::path::Path::new(&dir).join(soname).display(),
                std::path::Path::new(obj)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            );
        }
        eprintln!();
    }

    inject_libc_dep(env, root)
}

/// Real `_inject_libc_dep` (`doebuild.py:3026-3067`, bug #753500): every
/// ELF-bearing package records an implicit `>=<provider>` runtime dep on
/// the installed libc, so a binpkg merge cannot silently downgrade the
/// libc it was built against. `find_libc_deps(..., realized=True)`:
/// expand `virtual/libc`'s installed `RDEPEND` to its provider cps
/// (`portage_repo::libc_provider_cps`), take each provider's lowest
/// installed version (real `portdb.match(atom)[0]`), skip entirely when
/// this package *is* a libc provider (real's own `pkgcmp` self-check),
/// then append the `>=` atoms to the existing `build-info/RDEPEND`.
fn inject_libc_dep(env: &Environment, root: &Path) -> Result<(), String> {
    let current_cp = (env.category.clone(), env.split.pn.clone());
    let mut providers: Vec<(String, String)> =
        portage_repo::libc_provider_cps(root).into_iter().collect();
    providers.sort();
    let mut injected: Vec<String> = Vec::new();
    for (category, package) in providers {
        if (category.clone(), package.clone()) == current_cp {
            return Ok(());
        }
        let lowest = portage_repo::installed_candidates(root, &category, &package)
            .into_iter()
            .min_by(|a, b| portage_versions::vercmp(&a.0, &b.0).unwrap_or(0).cmp(&0));
        if let Some((version, _slot, _sub_slot)) = lowest {
            injected.push(format!(">={category}/{package}-{version}"));
        }
    }
    if injected.is_empty() {
        return Ok(());
    }
    let rdepend_path = env.build_info().join("RDEPEND");
    let existing = std::fs::read_to_string(&rdepend_path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let value = if existing.is_empty() {
        injected.join(" ")
    } else {
        format!("{existing} {}", injected.join(" "))
    };
    std::fs::write(&rdepend_path, format!("{value}\n"))
        .map_err(|e| format!("{}: {e}", rdepend_path.display()))?;
    Ok(())
}

/// Real `_post_src_install_uid_fix`'s own size accumulation: every
/// regular file under `dir`, once per inode (`counted_inodes`), summed
/// by `st_size`. Directory and symlink entries contribute nothing;
/// unreadable entries are an error (real would raise the same way).
fn dir_size_bytes(dir: &Path) -> Result<u64, String> {
    use std::os::unix::fs::MetadataExt;
    fn walk(dir: &Path, seen: &mut std::collections::HashSet<u64>) -> Result<u64, String> {
        let mut total = 0;
        let entries =
            portage_util::read_dir_entries(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries {
            let path = entry.path();
            let md =
                std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            if md.is_dir() {
                total += walk(&path, seen)?;
            } else if md.is_file() && seen.insert(md.ino()) {
                total += md.len();
            }
        }
        Ok(total)
    }
    walk(dir, &mut std::collections::HashSet::new())
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

/// Real `special_env_vars.environ_whitelist` + its `environ_whitelist_re`
/// (`^(CCACHE_|DISTCC_).*`): the calling-environment variables real
/// portage's `config.environ()` carries into a phase's environment.
/// portuale spawns phases with the full inherited process env plus its
/// `phase_env_vars` overrides; real never inherits wholesale, so vars
/// like `EMERGE_DEFAULT_OPTS` (an `emerge` CLI option, not a build var)
/// or a portuale-only `PORTAGE_RUNNING_ROOT` leaked through and, once
/// L1-c started regenerating the vdb `environment` from the live phase
/// env, into the vdb where real's has nothing (L1-f). `run_one_phase_*`
/// now filters the inherited env to this list; `phase_env_vars` still
/// sets everything portuale actually needs on top. Copied verbatim from
/// a live `special_env_vars.environ_whitelist` -- entries portuale never
/// sets simply won't be present to pass through.
const ENVIRON_WHITELIST: &[&str] = &[
    "A",
    "AA",
    "ACCEPT_LICENSE",
    "BASH_ENV",
    "BINPKG_FORMAT",
    "BROOT",
    "BUILD_ID",
    "BUILD_PREFIX",
    "CATEGORY",
    "COLORTERM",
    "COLUMNS",
    "CVS_RSH",
    "D",
    "DISPLAY",
    "DISTDIR",
    "DOC_SYMLINKS_DIR",
    "EAPI",
    "EBUILD",
    "EBUILD_FORCE_TEST",
    "EBUILD_PHASE",
    "EBUILD_PHASE_FUNC",
    "ECHANGELOG_USER",
    "ECLASSDIR",
    "ECLASS_DEPTH",
    "ED",
    "EDITOR",
    "EMERGE_FROM",
    "ENV_UNSET",
    "EPREFIX",
    "EROOT",
    "ESYSROOT",
    "FEATURES",
    "FILESDIR",
    "GPG_AGENT_INFO",
    "HOME",
    "INSTALL_MASK",
    "LANG",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_PAPER",
    "LC_TIME",
    "LD_PRELOAD",
    "LESS",
    "LESSOPEN",
    "LOGNAME",
    "LS_COLORS",
    "MAKEFLAGS",
    "MAKEOPTS",
    "MERGE_TYPE",
    "NINJAOPTS",
    "NOCOLOR",
    "NO_COLOR",
    "P",
    "PAGER",
    "PATH",
    "PF",
    "PKGDIR",
    "PKGUSE",
    "PKG_INSTALL_MASK",
    "PKG_LOGDIR",
    "PKG_TMPDIR",
    "PM_EBUILD_HOOK_DIR",
    "PN",
    "PORTAGE_ACTUAL_DISTDIR",
    "PORTAGE_ARCHLIST",
    "PORTAGE_BASHRC",
    "PORTAGE_BASHRC_FILES",
    "PORTAGE_BINPKG_FILE",
    "PORTAGE_BINPKG_TAR_OPTS",
    "PORTAGE_BINPKG_TMPFILE",
    "PORTAGE_BIN_PATH",
    "PORTAGE_BUILDDIR",
    "PORTAGE_BUILD_GROUP",
    "PORTAGE_BUILD_USER",
    "PORTAGE_BUNZIP2_COMMAND",
    "PORTAGE_BZIP2_COMMAND",
    "PORTAGE_COLORMAP",
    "PORTAGE_COMPRESS",
    "PORTAGE_COMPRESSION_COMMAND",
    "PORTAGE_COMPRESS_EXCLUDE_SUFFIXES",
    "PORTAGE_CONFIGROOT",
    "PORTAGE_DEBUG",
    "PORTAGE_DEPCACHEDIR",
    "PORTAGE_DOHTML_UNWARNED_SKIPPED_EXTENSIONS",
    "PORTAGE_DOHTML_UNWARNED_SKIPPED_FILES",
    "PORTAGE_DOHTML_WARN_ON_SKIPPED_FILES",
    "PORTAGE_EBUILD_EXIT_FILE",
    "PORTAGE_ECLASS_LOCATIONS",
    "PORTAGE_FEATURES",
    "PORTAGE_GID",
    "PORTAGE_GRPNAME",
    "PORTAGE_INST_GID",
    "PORTAGE_INST_UID",
    "PORTAGE_INTERNAL_CALLER",
    "PORTAGE_IPC_DAEMON",
    "PORTAGE_IUSE",
    "PORTAGE_LOG_FILE",
    "PORTAGE_OVERRIDE_EPREFIX",
    "PORTAGE_PIPE_FD",
    "PORTAGE_PROPERTIES",
    "PORTAGE_PYM_PATH",
    "PORTAGE_PYTHON",
    "PORTAGE_PYTHONPATH",
    "PORTAGE_QUIET",
    "PORTAGE_REPOSITORIES",
    "PORTAGE_REPO_NAME",
    "PORTAGE_REPO_REVISIONS",
    "PORTAGE_RESTRICT",
    "PORTAGE_SOCKS5_PROXY",
    "PORTAGE_TMPDIR",
    "PORTAGE_UPDATE_ENV",
    "PORTAGE_USERNAME",
    "PORTAGE_VERBOSE",
    "PORTAGE_WORKDIR_MODE",
    "PORTAGE_XATTR_EXCLUDE",
    "PORTDIR",
    "PORTDIR_OVERLAY",
    "PR",
    "PREROOTPATH",
    "PV",
    "PVR",
    "PYTHONDONTWRITEBYTECODE",
    "REPLACED_BY_VERSION",
    "REPLACING_VERSIONS",
    "ROOT",
    "ROOTPATH",
    "SANDBOX_LOG",
    "SSH_AGENT_PID",
    "SSH_AUTH_SOCK",
    "STY",
    "SYSROOT",
    "T",
    "TEMP",
    "TERM",
    "TERMCAP",
    "TMP",
    "TMPDIR",
    "USER",
    "USE_EXPAND",
    "USE_ORDER",
    "WINDOW",
    "WORKDIR",
    "XARGS",
    "XAUTHORITY",
    "__PORTAGE_TEST_HARDLINK_LOCKS",
    "ftp_proxy",
    "http_proxy",
    "https_proxy",
    "no_proxy",
];

/// `key` survives real portage's `config.environ()` calling-env filter.
pub(crate) fn environ_whitelisted(key: &str) -> bool {
    ENVIRON_WHITELIST.contains(&key) || key.starts_with("CCACHE_") || key.starts_with("DISTCC_")
}

/// The `FEATURES` string a phase-execution decision should consult:
/// the **last** `FEATURES` pair on `extra_env` when present (the
/// resolved incremental list #37 S2 threads), else the process
/// environment (a standalone `ebuild <file>` run, where no resolved
/// config exists). Real semantics: every Rust-side gate below reads
/// `settings.features`, i.e. the resolved list, never the raw calling
/// env.
fn features_string(extra_env: &[(String, String)]) -> String {
    extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "FEATURES")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| std::env::var("FEATURES").unwrap_or_default())
}

/// A `FEATURES` token check over the given resolved features string
/// (`features_string`'s output). The `pretend.rs`/`ebuild_merge.rs`
/// siblings keep their own process-env reads: they are CLI-boundary
/// fallbacks (`ebuild <file>`), not phase execution.
fn feature_token_present(features: &str, token: &str) -> bool {
    features.split_whitespace().any(|t| t == token)
}

/// Real `FEATURES` for the phase's own bash environment
/// (`doebuild_environment()`'s own `mysettings["FEATURES"]`, the same
/// source every isolation check above already reads via
/// `feature_token_present`) -- passed straight through rather than
/// blanked, so real, unmodified bash that gates its own behavior on a
/// `contains_word <token> "${FEATURES}"` check runs correctly: real
/// `bin/estrip`'s own `compressdebug`/`installsources`/`nostrip`/
/// `splitdebug`/`xattr` handling (called from `prepstrip`/`prepallstrip`
/// during `install`); `phase-functions.sh`'s own `__dyn_test` (`FEATURES=
/// test` is what actually makes `src_test` run at all -- previously a
/// silent, unconditional no-op here regardless of what the invoker
/// asked for) and `nostrip`/`ccache`/`distcc`/`noauto` checks;
/// `misc-functions.sh`'s own `noclean`/`keepwork`/`sfperms`/`suidctl`/
/// `selinux`/`packdebug`/`chflags`/`binpkg-do{compress,strip}` handling.
/// None of these are also interpreted by portuale's own Rust side (the
/// tokens Rust *does* independently act on --
/// `sandbox`/`usersandbox`/`{network,ipc,mount,pid}-sandbox`,
/// `distlocks`, `buildpkg-live`, `binpkg-multi-instance` -- are never
/// gated on again by any real bash this module runs, confirmed by
/// grepping every `contains_word … "${FEATURES}"` site in `bin/*.sh`),
/// so there's no double-handling risk; a token real bash doesn't
/// understand, or whose supporting tool/env var isn't present
/// (`ccache`/`distcc`/`selinux` all require their own binaries), is
/// exactly as inert as it always was, and degrades exactly the way real
/// portage's own bash would under the same conditions.
fn phase_features_value() -> String {
    std::env::var("FEATURES").unwrap_or_default()
}

/// `FEATURES=network-sandbox` present?
fn network_sandbox_requested(features: &str) -> bool {
    feature_token_present(features, "network-sandbox")
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
fn fs_sandbox_requested(features: &str) -> bool {
    feature_token_present(features, "sandbox") || feature_token_present(features, "usersandbox")
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
fn fs_sandbox_for_phase(phase: &str, features: &str) -> bool {
    use std::sync::OnceLock;
    if !fs_sandbox_requested(features) || !SANDBOXED_SRC_PHASES.contains(&phase) {
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
fn phase_isolation(env: &Environment, phase: &str, features: &str) -> Isolation {
    use std::sync::OnceLock;
    if !SANDBOXED_SRC_PHASES.contains(&phase) {
        return Isolation::default();
    }
    let mut net = network_sandbox_requested(features);
    if net {
        // Exemption checks run for sandboxable phases only (`depend`
        // never reaches here -- it is outside `SANDBOXED_SRC_PHASES`
        // above), so the empty-set reduction is the whole story.
        let (restrict, properties) =
            restrict_and_properties(env, &std::collections::HashSet::new());
        if network_sandbox_exempt(phase, &restrict, &properties) {
            net = false;
        }
    }
    let mut iso = Isolation {
        net,
        ipc: feature_token_present(features, "ipc-sandbox"),
        mount: feature_token_present(features, "mount-sandbox"),
        pid: feature_token_present(features, "pid-sandbox"),
        fs_sandbox: fs_sandbox_for_phase(phase, features),
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
///   plus the real `AI_ADDRCONFIG`-workaround addresses (real
///   `_configure_loopback_interface`, `process.py:838-871`; bug #690758
///   -- some glibc `getaddrinfo()` calls with `AI_ADDRCONFIG` set return
///   no results at all when a family has zero non-loopback addresses
///   configured, so real adds one anyway) for `--net`, `mount
///   --make-rslave /` for `--mount` (real `_exec2`'s own call) -- then
///   `exec "$@"` runs the real (possibly `sandbox`-prefixed) command.
///   The IPv6 address add is allowed to fail silently (`2>/dev/null`,
///   same as every other command in this shim) exactly the way real's
///   own `has_ipv6()` guard skips it outright on an IPv6-less host --
///   the net effect (no IPv6 loopback address) is identical either way.
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
        shim.push_str(
            "ip link set lo up 2>/dev/null; \
             ip addr add 10.0.0.1/8 dev lo 2>/dev/null; \
             ip -6 addr add fd::1/8 dev lo 2>/dev/null; ",
        );
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
    let d = eapi_path_var(&env.eapi, &format!("{}/", env.d().display()));
    let root_value = eapi_path_var(&env.eapi, &format!("{}/", root.display()));
    let resolved_features = features_string(extra_env);
    let path = phase_path(helpers_dir, extra_env);
    // Config-derived base env for standalone runs (USE + the whole
    // resolved config env), computed once here so the single config load
    // serves both the `USE` entry below and the base pairs seeding
    // `vars`. `("", [])` for merge builds (their `extra_env` carries
    // everything) and the `depend` phase -- see the helper.
    let standalone_base_env =
        phase_standalone_base_env(env, config_root, root, ebuild_phase_value, extra_env);
    // Real `doebuild_environment()` assigns its computed values *after*
    // the config is built, so they win over a same-named config key:
    // the resolved-config base pairs seed `vars` first and the computed
    // literal below overrides them (downstream `cmd.envs` is last-wins),
    // while the merge path's `extra_env` still overrides both at the end.
    let mut vars: Vec<(String, String)> = standalone_base_env.1;
    vars.extend(vec![
        ("EAPI".to_string(), env.eapi.clone()),
        ("PN".to_string(), env.split.pn.clone()),
        ("PV".to_string(), env.split.pv.clone()),
        ("PR".to_string(), env.split.pr.clone()),
        ("PVR".to_string(), env.split.pvr.clone()),
        ("P".to_string(), env.split.p.clone()),
        ("PF".to_string(), env.split.pf.clone()),
        ("CATEGORY".to_string(), env.category.clone()),
        ("EBUILD".to_string(), env.ebuild_abs.display().to_string()),
        ("ROOT".to_string(), root_value.clone()),
        ("EROOT".to_string(), root_value),
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
            if fs_sandbox_for_phase(ebuild_phase_value, &resolved_features) {
                "0"
            } else {
                "1"
            }
            .to_string(),
        ),
        ("FEATURES".to_string(), phase_features_value()),
        // Real `doebuild_environment()` exports the package's effective
        // `USE` into every phase. Merge builds override this base value
        // downstream via `extra_env` (see `run_commands_async`); a
        // standalone `ebuild <file> <phase>` has no `extra_env` flags, so
        // the config-derived base is what its `use()` calls see.
        // `phase_standalone_base_env` returns `("", [])` for the
        // `depend` phase and whenever `extra_env` already carries `USE`.
        ("USE".to_string(), standalone_base_env.0.clone()),
        ("EPREFIX".to_string(), String::new()),
        ("EMERGE_FROM".to_string(), "ebuild".to_string()),
        ("PORTAGE_QUIET".to_string(), "1".to_string()),
        (
            "PORTAGE_DEBUG".to_string(),
            if debug { "1" } else { "0" }.to_string(),
        ),
        ("EBUILD_PHASE".to_string(), ebuild_phase_value.to_string()),
    ]);

    // Real `doebuild_environment()`: `PORTAGE_RESTRICT`/
    // `PORTAGE_PROPERTIES` are set for every phase, always, from the
    // package's own already-USE-reduced `RESTRICT`/`PROPERTIES` (real
    // `str(self._pkg.restrict)`/`str(self._pkg.properties)`). Real bash
    // consults these directly -- `phase-functions.sh:549`'s own
    // `RESTRICT=test` skip, `:777`'s `RESTRICT=nostrip`/
    // `RESTRICT=strip` -- rather than re-deriving them from `RESTRICT`/
    // `PROPERTIES` itself, which portuale's own phase env never exports
    // at all (only the ebuild's own bash sees the raw, unreduced values,
    // via its own sourced metadata). The `depend` phase reduces on the
    // config `USE` set (real `doebuild(mydo="depend")`'s own
    // `PORTAGE_USE`); every other phase reduces on the empty set -- see
    // `restrict_and_properties`'s own doc comment.
    let restrict_use_set = if ebuild_phase_value == "depend" {
        depend_use_set(env, config_root, root)
    } else {
        std::collections::HashSet::new()
    };
    let (restrict, properties) = restrict_and_properties(env, &restrict_use_set);
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

    // `PATH` is consumed above as the base behind the helper dirs; a
    // verbatim `extra_env` pair would drop them again.
    vars.extend(extra_env.iter().filter(|(k, _)| k != "PATH").cloned());

    // Real `EbuildPhase._start` (`EbuildPhase.py:52-56`) calls
    // `split_LC_ALL(settings)` (`portage/util/locale.py:160`) before
    // spawning any phase: a set `LC_ALL` is copied to every
    // `locale_categories` entry and itself blanked (then deleted by
    // `config.environ()`; for EAPI 5+'s `posixish_locale` real even
    // asserts it is absent, `config.py:3374-3385`). The phase therefore
    // sees `LC_*`, never `LC_ALL`. The resolved `extra_env` value wins
    // over the calling process env, and any `LC_ALL` pair is filtered
    // out so it cannot leak back in. (The `--shell brush` path still
    // inherits the hosting process's own `LC_ALL` in its embedded
    // shell; the category exports below override it for every
    // subprocess the phase spawns.)
    vars.retain(|(k, _)| k != "LC_ALL");
    let lc_all = extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "LC_ALL")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("LC_ALL").ok())
        .unwrap_or_default();
    if !lc_all.is_empty() {
        for category in [
            "LC_COLLATE",
            "LC_CTYPE",
            "LC_MONETARY",
            "LC_MESSAGES",
            "LC_NUMERIC",
            "LC_TIME",
            "LC_ADDRESS",
            "LC_IDENTIFICATION",
            "LC_MEASUREMENT",
            "LC_NAME",
            "LC_PAPER",
            "LC_TELEPHONE",
        ] {
            vars.push((category.to_string(), lc_all.clone()));
        }
    }

    vars
}

/// Real `config.environ()`'s path-variable shape (`config.py:3392-3395`):
/// `D`/`ED`/`ROOT`/`EROOT` end with a trailing `/` only for EAPI <= 6
/// (`eapi.py:307`, `path_variables_end_with_trailing_slash`); later
/// EAPIs `rstrip("/")` them, so `ROOT=/` is exported as `""` and
/// `${ROOT}/usr/src/linux` (`linux-info.eclass`) stays `/usr/src/linux`.
/// `value` is passed with its trailing slash.
fn eapi_path_var(eapi: &str, value: &str) -> String {
    let trailing = matches!(eapi, "0" | "1" | "2" | "3" | "4" | "5" | "6");
    let base = value.trim_end_matches('/');
    if trailing {
        format!("{base}/")
    } else {
        base.to_string()
    }
}

/// Real `_doebuild_path` (`doebuild.py:332-378`), narrowed to the
/// `ebuild-helpers` prefix portuale's runner needs: the helper dir first,
/// then every entry of the base `PATH` not already listed. The base is
/// the resolved config's `PATH` when the caller threaded one (the last
/// `extra_env` pair -- `portage_profile::phase_environ` exports it only
/// when `env.d` sets `PATH`, real's "ignore PATH from the calling
/// environment" rule), else the calling env's.
fn phase_path(helpers_dir: &Path, extra_env: &[(String, String)]) -> String {
    let base = extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    let helpers = helpers_dir.display().to_string();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    seen.insert(helpers.as_str());
    let mut parts = vec![helpers.as_str()];
    for p in base.split(':') {
        if seen.insert(p) {
            parts.push(p);
        }
    }
    parts.join(":")
}

/// `Brush`-backend-only: `phase_env_vars` formatted as real `export
/// NAME=value` bash source text. Values are **single-quote escaped**
/// (`shell_single_quote`, G6 of the #37 plan): since the resolved
/// config env reaches `extra_env`, a value can now be arbitrary
/// config/ebuild text (`CFLAGS` with `$(…)`, `PORTAGE_COMPRESS_
/// EXCLUDE_SUFFIXES` globs, descriptions with quotes), and the previous
/// `{:?}`-formatted double-quoted export would have let the brush shell
/// word-split/expand/command-substitute it. `Bash` backend needs no such
/// text at all -- `phase_env_vars`'s own pairs are passed directly as
/// real subprocess environment variables instead, see
/// `run_one_phase_bash`/`run_misc_functions_bash`.
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
    let mut script = String::new();
    // The embedded brush shell starts from the inherited process env
    // (like the bash backend's subprocess). Real `doebuild` never
    // inherits wholesale -- drop every calling-env var real's
    // `environ_whitelist` wouldn't keep, before the `phase_env_vars`
    // exports set what portuale needs (L1-f, mirrors `run_one_phase_bash`).
    let set_here: std::collections::HashSet<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
    for (name, _) in std::env::vars() {
        if !environ_whitelisted(&name)
            && !set_here.contains(name.as_str())
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !name.is_empty()
            && !name.as_bytes()[0].is_ascii_digit()
        {
            script.push_str(&format!("unset {name}\n"));
        }
    }
    for (name, value) in vars {
        script.push_str(&format!("export {name}={}\n", shell_single_quote(&value)));
    }
    script
}

/// A POSIX single-quoted shell word: everything between `'…'` is
/// literal, and an embedded `'` is the usual `'\''` close/escape/reopen
/// dance. `$`, backticks, `\`, `"`, whitespace and newlines are all
/// inert inside it. Used by `phase_setup_script` for every brush
/// `export` value (G6 of the #37 plan).
fn shell_single_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
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
    let iso = phase_isolation(env, phase, &features_string(extra_env));
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

/// The host's real `bash`, as a phase's own `$BASH`.
///
/// `__filter_readonly_variables` (`bin/phase-functions.sh:101`) lists
/// bash's special variables by running `env -i -- "${BASH}" -c '…'`; a
/// real bash sets `$BASH` to itself, while an embedded brush `Shell` is
/// built without a `shell_name`, so brush never sets it and the command
/// comes back empty (the `env: '': No such file or directory` in the L2
/// smoke log). Resolve the first `bash` on `PATH` -- the same one the
/// `Bash` backend spawns -- once per process.
fn real_bash_path() -> String {
    use std::sync::OnceLock;
    static RESOLVED: OnceLock<String> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            if let Some(path) = std::env::var_os("PATH") {
                for dir in std::env::split_paths(&path) {
                    let candidate = dir.join("bash");
                    if candidate.is_file() {
                        return candidate.to_string_lossy().into_owned();
                    }
                }
            }
            "bash".to_owned()
        })
        .clone()
}

/// Builds one fresh embedded brush shell for a phase or misc-functions
/// run, with `$BASH` pointed at the host's real bash (`real_bash_path`)
/// so the real `bin/*.sh` construct of listing bash's special variables
/// works. `BASH` is set by a shell itself, not taken from the
/// environment, so this is a shell-state assignment, not an exported
/// variable (`phase_setup_script` is not involved).
async fn new_brush_phase_shell() -> Result<brush_core::Shell, String> {
    let mut shell = brush_core::Shell::builder()
        .default_builtins(brush_builtins::BuiltinSet::BashMode)
        .build()
        .await
        .map_err(|e| format!("brush shell failed to start: {e}"))?;
    shell
        .env_mut()
        .set_global("BASH", brush_core::ShellVariable::new(real_bash_path()))
        .map_err(|e| format!("setting $BASH failed: {e}"))?;
    Ok(shell)
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
    let mut shell = new_brush_phase_shell().await?;
    let (params, pump) = brush_phase_params(&mut shell, log_file)?;

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
    // The shell owns the pipe write ends (its fd table) and must drop
    // before `pump` joins (EOF then finish) -- see `open_log_file`'s
    // lifetime contract. No `?` may return between here and the drops
    // (an early return would drop `pump`, declared after `shell`,
    // first and hang the join), so the outcome is captured, the
    // holders dropped in order, and only then returned.
    let outcome: Result<i32, String> = async {
        let setup_result = shell
            .run_string(&setup, &brush_core::SourceInfo::default(), &params)
            .await
            .map_err(|e| format!("environment setup failed: {e}"))?;
        // A non-zero result here is a real setup failure, not a phase
        // outcome: carry on only when the environment actually loaded. The
        // exit code (not just a printed error) is what makes this a hard
        // stop -- a broken saved environment must fail the phase, never
        // silently skip the ebuild's own functions (see B2 of
        // `history/backlog_tier_1_sliced.opus.md`). `Ok(nonzero)`, like
        // `run_one_phase_bash`'s own child status: a phase failure, not a
        // spawn failure.
        if !setup_result.is_success() {
            return Ok(i32::from(u8::from(setup_result.exit_code)));
        }

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
        let source_result = shell
            .source_script(
                bin_dir.join("ebuild.sh"),
                std::iter::empty::<String>(),
                &params,
            )
            .await
            .map_err(|e| format!("sourcing bin/ebuild.sh failed: {e}"))?;
        // A non-zero exit from sourcing ebuild.sh means its own top-level
        // guard died -- most importantly the `source "${T}"/environment ||
        // die "error sourcing environment"` at `bin/ebuild.sh:580` -- and
        // `__ebuild_main` must not run on a half-loaded environment. Same
        // phase-failure shape as above.
        if !source_result.is_success() {
            return Ok(i32::from(u8::from(source_result.exit_code)));
        }

        shell
            .invoke_function("__ebuild_main", [phase], params)
            .await
            .map_err(|e| format!("phase {phase} failed: {e}"))
            .map(|result| i32::from(u8::from(result.exit_code)))
    }
    .await;
    drop(shell);
    drop(pump);
    outcome
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
    // The gzip pump guard must outlive `cmd` (see `open_log_file`'s
    // lifetime contract + the sink comment below): it joins the gzip
    // thread, which needs EOF, which needs `cmd`'s stored parent
    // write-end copies closed. Declared BEFORE `cmd` so reverse drop
    // order destroys it AFTER `cmd` on every path (normal return and
    // `?` early return alike); `drop(cmd)` below closes the copies
    // deterministically right after the wait.
    let mut pump_guard: Option<LogPump> = None;
    let mut cmd = sandbox_wrapped_command(&bin_dir.join("ebuild.sh"), phase, iso);
    // Real `doebuild` spawns a phase with a curated `config.environ()`,
    // never the inherited process env wholesale -- so `EMERGE_DEFAULT_OPTS`
    // and portuale-only vars don't leak into the phase (and, since L1-c,
    // into the regenerated vdb `environment`). Keep only the calling-env
    // vars real's `environ_whitelist` keeps; `phase_env_vars` sets the
    // rest on top. (L1-f.)
    cmd.env_clear();
    // `LC_ALL` never reaches a real phase (see `phase_env_vars`' own
    // locale split): real `split_LC_ALL` blanks it, so even a
    // whitelisted process-env entry is dropped here and the `LC_*`
    // category exports stand alone.
    cmd.envs(std::env::vars().filter(|(k, _)| environ_whitelisted(k) && k != "LC_ALL"));
    cmd.envs(vars);
    if let Some(path) = log_file {
        // `sink.pump` MUST outlive `cmd`: the guard joins the gzip
        // thread, which needs EOF on the pipes, which needs `cmd`'s
        // stored parent copies of the write ends closed (plus the
        // waited child's). A guard scoped to this block would drop
        // (join) here -- before the child is even spawned, with `cmd`
        // still holding its copies -- and hang forever. So only the
        // write ends move into `cmd`; the guard escapes to the outer
        // scope, where reverse declaration order drops it after `cmd`
        // on every path (normal return and `?` early return alike).
        // `drop(cmd)` below closes the copies deterministically right
        // after the wait; see `open_log_file`.
        let sink = open_log_file(path)?;
        cmd.stdout(sink.out).stderr(sink.err);
        pump_guard = sink.pump;
    }
    let status = spawn_trackable(&mut cmd)
        .map_err(|e| format!("spawning real bash for phase {phase} failed: {e}"))?;
    drop(cmd);
    // Explicit (a pure-drop guard reads as unused otherwise): joins the
    // gzip thread now that every write end is closed. Scope-end order
    // would do the same, including on the `?` early return above.
    drop(pump_guard);
    Ok(status.code().unwrap_or(1))
}

// Real `Scheduler._terminate_tasks`'s own "send kill signals... send
// kill signals and return without waiting for exit status"
// (`PollScheduler.py:106-118`, called once `_keep_scheduling` sees any
// package fail without `--keep-going`): every scheduled build's own
// top-level real subprocess (a phase's `unshare [sandbox] bash
// bin/ebuild.sh <phase>`, or `bin/misc-functions.sh`'s own equivalent)
// is spawned into a fresh process group (`process_group(0)`, `setpgid`
// before `exec`, so the child's own pid becomes the group id) via
// `spawn_trackable`, registered in `registry` for as long as the
// process runs. A whole process GROUP, not just the one pid: real
// portage's own descendant tree here -- `unshare` -> `sandbox` -> real
// `bash` -> whatever the ebuild itself spawns, e.g. a real compiler --
// all inherit the same group unless one of them starts a session of
// its own (none of these do), so a single group-directed signal
// (`kill(-pgid, …)`) reaches the whole tree, not just its outermost
// process.
//
// `registry` is per-`run_build_scheduler`-*call*, not a single
// process-wide singleton: `emerge_build::run_build_scheduler` creates
// one fresh (`new_scheduler_registry`) and shares it with its own
// worker threads only (`scope_scheduler_registry`'s own RAII guard,
// set at the very top of each `std::thread::scope`-spawned closure).
// A thread-local pointer to it -- not a parameter threaded through
// every intervening call (`run_commands_logged`/`run_one_phase`/…) --
// keeps this from being a signature change touching that whole chain;
// safe specifically because none of this module's own async phase
// bodies ever `tokio::spawn` an inner task (confirmed by grep), so
// `runtime.block_on(fut)` always drives the whole future to completion
// on the *calling* OS thread -- the same one the scheduler's own
// `scope.spawn` closure set the guard on, with no tokio-internal
// worker-thread migration to break the thread-local's visibility.
// Any other thread (a concurrent, unrelated test in the same process,
// a standalone `ebuild <file> <phase>` run, a single-job non-scheduler
// build) simply never sets the guard, so its own spawns are never
// registered anywhere and `kill_registered_children` can never reach
// them -- this is what makes a single global registry unsafe (an
// unrelated `cargo test` thread's subprocess could get killed by a
// completely different test's own scheduler-failure path) and why
// this is scoped per-call instead.
thread_local! {
    /// The current thread's scheduler registry, if any -- see the
    /// module comment right above for the full grounding.
    static SCHEDULER_REGISTRY: std::cell::RefCell<Option<std::sync::Arc<std::sync::Mutex<std::collections::HashSet<i32>>>>> =
        const { std::cell::RefCell::new(None) };
}

/// A fresh, empty registry for one `run_build_scheduler` call.
pub(crate) fn new_scheduler_registry()
-> std::sync::Arc<std::sync::Mutex<std::collections::HashSet<i32>>> {
    std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// RAII guard: while alive, every `spawn_trackable` call on *this*
/// thread registers into `registry`. Restores whatever was set before
/// (normally `None`) on drop, including on an early return or panic --
/// see `SCHEDULER_REGISTRY`'s own doc comment for why this is scoped
/// per-thread rather than reaching for one process-wide singleton.
pub(crate) struct SchedulerRegistryGuard(
    Option<std::sync::Arc<std::sync::Mutex<std::collections::HashSet<i32>>>>,
);

impl Drop for SchedulerRegistryGuard {
    fn drop(&mut self) {
        SCHEDULER_REGISTRY.with(|cell| *cell.borrow_mut() = self.0.take());
    }
}

pub(crate) fn scope_scheduler_registry(
    registry: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<i32>>>,
) -> SchedulerRegistryGuard {
    let previous = SCHEDULER_REGISTRY.with(|cell| cell.borrow_mut().replace(registry));
    SchedulerRegistryGuard(previous)
}

/// `SIGTERM`s every process group currently registered in `registry`.
/// Best-effort: a group that already exited between the snapshot and
/// the signal simply yields `ESRCH`, silently ignored, matching real's
/// own "typically... return without waiting for exit status" non-
/// blocking semantics.
pub(crate) fn kill_registered_children(
    registry: &std::sync::Mutex<std::collections::HashSet<i32>>,
) {
    let pgids: Vec<i32> = registry.lock().unwrap().iter().copied().collect();
    for pgid in pgids {
        // SAFETY: `kill` takes only plain ints (no pointers); `pgid` is a
        // process-group id recorded by this process itself, so negating it
        // cannot overflow and names a group we legitimately own.
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
    }
}

/// `cmd.status()`, but registered in the calling thread's own
/// `SCHEDULER_REGISTRY` (if any -- see that thread-local's own doc
/// comment) for the duration. Every real top-level subprocess a
/// scheduled build's own phase/misc-functions execution spawns goes
/// through this instead of a bare `.status()`.
fn spawn_trackable(cmd: &mut std::process::Command) -> std::io::Result<std::process::ExitStatus> {
    use std::os::unix::process::CommandExt as _;
    cmd.process_group(0);
    let mut child = cmd.spawn()?;
    let pgid = child.id() as i32;
    let registry = SCHEDULER_REGISTRY.with(|cell| cell.borrow().clone());
    if let Some(reg) = &registry {
        reg.lock().unwrap().insert(pgid);
    }
    let status = child.wait();
    if let Some(reg) = &registry {
        reg.lock().unwrap().remove(&pgid);
    }
    status
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
/// An exit-code-carrying `run_depend_phase` failure. Real
/// `EbuildMetadataPhase` reports every outcome as a `returncode`
/// (`os.EX_OK` / 1 for an expected failure like invalid metadata /
/// anything else for an unexpected one), and `MetadataRegen._task_exit`
/// keys its `cp_retry` decision off exactly that (`returncode != 1` is
/// retried). Threading the code through -- instead of a bare message --
/// is what lets `regen.rs` replay that decision.
pub(crate) struct DependError {
    pub(crate) code: i32,
    pub(crate) message: String,
}

pub(crate) fn run_depend_phase(
    env: &Environment,
    root: &Path,
    config_root: &Path,
    debug: bool,
) -> Result<std::collections::HashMap<String, String>, DependError> {
    let bin_dir = bin_dir().to_path_buf();
    let helpers_dir = bin_dir.join("ebuild-helpers");
    // Builddir setup happens before any phase spawns -- real's
    // `doebuild`-before-spawn int retval, code 1, never retried.
    create_directories(env).map_err(|e| DependError {
        code: 1,
        message: e,
    })?;

    // Real `EbuildMetadataPhase` reads the metadata from a private pipe,
    // so concurrent `emerge` processes never share it. The builddir is
    // shared (`${PORTAGE_TMPDIR}/portage/<cat>/<pf>`), so the file that
    // stands in for the pipe is per-process: a fixed name let two
    // concurrent cache-miss resolutions delete each other's metadata
    // and report the package as having no ebuilds.
    let meta_path = env
        .t()
        .join(format!(".depend-metadata.{}", std::process::id()));
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
    let out = cmd.output().map_err(|e| DependError {
        // Real `EbuildMetadataPhase._async_start`: `doebuild` failing
        // before it even spawns surfaces as an int retval (in
        // practice 1) -- an "expected" failure, never retried by
        // `metadata_regen_retry`'s `cp_retry` (which only re-runs a
        // cp whose phase died with an *unexpected* returncode).
        code: 1,
        message: format!("spawning bash for the depend phase failed: {e}"),
    })?;
    if !out.status.success() {
        let code = out.status.code().unwrap_or(-1);
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
        return Err(DependError {
            // The phase's own exit code passes through verbatim, exactly
            // like real `EbuildMetadataPhase`'s `returncode` (real only
            // rewrites it to 2 on a sandbox-log hit -- and the `depend`
            // phase is never sandboxed, real `_doebuild_spawn`'s
            // `SANDBOXED_SRC_PHASES` excludes it -- so no rewrite here).
            code,
            message: format!(
                "depend phase failed for {}/{} (exit {code}):\n{tail}",
                env.category, env.split.pf,
            ),
        });
    }

    let text = std::fs::read_to_string(&meta_path).map_err(|e| DependError {
        // The phase exited 0 but left no parseable metadata behind --
        // real's `metadata_valid == False` arm, which sets returncode 1
        // (an "expected" failure, never retried).
        code: 1,
        message: format!("{}: {e}", meta_path.display()),
    })?;
    let _ = std::fs::remove_file(&meta_path);
    Ok(parse_aux_entry(&text))
}

/// C2's registered cache-miss provider (see
/// `portage_repo::register_aux_metadata_provider`): regenerate one
/// ebuild's aux metadata by running its real `depend` phase -- real
/// `porttree.py`'s ebuild fallback when `metadata/md5-cache` has no
/// entry (`_pull_valid_cache` miss -> `doebuild(mydo="depend")`, C0's
/// oracle). The result is the same `HashMap` shape `read_md5_cache`
/// returns, so the resolver cannot tell the difference.
///
/// C3 adds the **depcachedir rung** around that phase run, exactly where
/// real `_pull_valid_cache` has it (`porttree.py:603-651`): before
/// running the phase, a depcachedir entry whose `_md5_` still matches
/// the ebuild is used as-is; after a successful run, the result is
/// written to the depcachedir (real `EbuildMetadataPhase` ->
/// `portdb._write_cache`, `porttree.py:578-596`) in real's flat layout
/// (`<depcachedir>/<repo path without the leading '/'>/<cat>/<pf>`,
/// `cache/flat_hash.py:20-22`). When that write cannot happen -- an
/// unwritable depcachedir, real's `secpass < 1` read-only branch -- the
/// result still lives for the process in `repo_aux_metadata`'s own memo
/// (C2). The write is best-effort; render/write live in `regen.rs`
/// because `--regen` writes the identical `flat_hash` format into the
/// repo's `metadata/md5-cache`, and the shared validator lives in
/// `portage_repo::md5_dict` (#46 S2).
///
/// Layering: this lives in `portuale` (the binary that owns real phase
/// execution) and is registered once from `main`; `portage-repo` only
/// holds the `fn`-pointer slot. `root`/`config_root`/`PORTAGE_TMPDIR`
/// come from the process environment, the same CLI-boundary defaults
/// `root_from_env`/`config_root_from_env`/`portage_tmpdir_from_env`
/// already establish. `depend` is never sandboxed, so this is a plain
/// `bash bin/ebuild.sh depend` (see `run_depend_phase`).
pub(crate) fn depend_phase_metadata(
    repo_location: &Path,
    category: &str,
    pf: &str,
) -> Result<std::collections::HashMap<String, String>, String> {
    let Some((package, _version)) = crate::remote_bundle::split_pf(pf) else {
        return Err(format!("{pf}: not a package-version name"));
    };
    let ebuild = repo_location
        .join(category)
        .join(&package)
        .join(format!("{pf}.ebuild"));
    if !ebuild.is_file() {
        return Err(format!("{}: no ebuild", ebuild.display()));
    }
    let config_root = portage_repo::config_root_from_env();
    let root = portage_repo::root_from_env();
    let masters = portage_repo::md5_dict::repo_masters_for_location(repo_location);

    // Real `_pull_valid_cache`: the depcachedir auxdb is consulted after
    // the repo's pregen `metadata/md5-cache` (C1's `read_md5_cache`) and
    // before `EbuildMetadataPhase` runs. Same validator as the repo
    // cache (real `template.validate_entry`), but the writable
    // depcachedir is `mtime_md5_database`: its `_eclasses_` carries the
    // eclass dir (`name\tpath\tmd5`, `store_eclass_paths = True`, S0
    // cell (a)).
    let depcache = depcache_entry_path(repo_location, category, pf);
    if let Ok(text) = std::fs::read_to_string(&depcache)
        && portage_repo::md5_dict::entry_is_valid(&text, &ebuild, repo_location, &masters, true)
    {
        return Ok(parse_aux_entry(&text));
    }

    // Real `EbuildMetadataPhase._async_start` (`:61-75`): the ebuild
    // head's EAPI is parsed before anything runs, and an unsupported one
    // skips the phase entirely -- the package is then masked by EAPI
    // visibility (S0 cell (e): `masked by: EAPI 9999`). Portuale returns
    // a failure instead of real's synthesized `{"EAPI": ...}` map (a
    // documented narrowing): the cp is dropped from candidates rather
    // than rendered as an EAPI mask.
    let head_eapi = parse_eapi(&std::fs::read_to_string(&ebuild).unwrap_or_default());
    let head_eapi = if head_eapi.is_empty() {
        "0".to_string()
    } else {
        head_eapi
    };
    if !portage_repo::md5_dict::eapi_is_supported(&head_eapi) {
        return Err(format!(
            "{}: EAPI {head_eapi} is unsupported",
            ebuild.display()
        ));
    }

    let portage_tmpdir = portage_repo::portage_tmpdir_from_env();
    let env = compute_environment(&ebuild, &portage_tmpdir)?;
    let md = run_depend_phase(&env, &root, &config_root, false).map_err(|e| e.message)?;

    // Real `_write_cache`: `metadata["_md5_"] = ebuild_hash.md5` then
    // `cache[cpv] = metadata` -- the md5-dict/`flat_hash` writer.
    // Best-effort: a read-only depcachedir keeps the result in memory
    // (C2's memo) exactly like real's volatile `_ro_auxdb` branch.
    if let Ok(body) = crate::regen::render_entry(&md, &ebuild, repo_location, &masters, true)
        && let Some(dir) = depcache.parent()
    {
        let _ = crate::regen::write_entry(dir, pf, &body);
    }
    Ok(md)
}

/// Split the `KEY=value` lines the `depend` phase pipe (and a
/// depcachedir entry) carries into the map `read_md5_cache` also
/// returns. Lines without `=` are skipped (real's
/// `_parse_data` raises `CacheCorruption` for those from the cache
/// reader; the phase pipe is written by `bin/ebuild.sh` and never emits
/// one).
fn parse_aux_entry(text: &str) -> std::collections::HashMap<String, String> {
    let mut md = std::collections::HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            md.insert(k.to_string(), v.to_string());
        }
    }
    md
}

/// Real `flat_hash.FsBased.__init__` (`cache/flat_hash.py:20-22`) with
/// the `md5_database`/`md5-dict` label: `os.path.join(depcachedir,
/// repo_path.lstrip('/').rstrip('/'))/<category>/<pf>` -- a flat tree
/// keyed by the repo's absolute path minus the leading separator (C0's
/// oracle: `/var/cache/edb/dep/var/db/repos/porttest/porttest/docs-1.0`).
fn depcache_entry_path(repo_location: &Path, category: &str, pf: &str) -> std::path::PathBuf {
    let location = repo_location.to_string_lossy();
    depcachedir()
        .join(location.trim_start_matches('/').trim_end_matches('/'))
        .join(category)
        .join(pf)
}

/// Real `settings.depcachedir` (`config.py:1114-1128`): the
/// `PORTAGE_DEPCACHEDIR` setting when present, else
/// `portage.const.DEPCACHE_PATH` = `/var/cache/edb/dep`. Not relative to
/// `ROOT` (real rebases it under `eroot` only in the unprivileged
/// non-`/` target-root fallback, which portuale's root runs never take).
/// Narrowing: real resolves the setting through the full config stack
/// (so a `make.conf` assignment wins over the environment); portuale
/// reads the process environment only, the same CLI-boundary default
/// the phase env's own whitelist (`ENVIRON_WHITELIST`) already carries.
fn depcachedir() -> std::path::PathBuf {
    std::env::var_os("PORTAGE_DEPCACHEDIR")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/cache/edb/dep"))
}

/// Real `bin/misc-functions.sh`'s own invocation shape -- unlike
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
    let fs_sandbox =
        fs_sandbox_requested(&features_string(extra_env)) && sandbox_binary().is_some();
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
    let mut shell = new_brush_phase_shell().await?;
    let (params, pump) = brush_phase_params(&mut shell, log_file)?;

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
    // Same shell-before-pump drop discipline as
    // `run_one_phase_brush`: capture the outcome, drop in order, then
    // return -- see `open_log_file`.
    let outcome: Result<i32, String> = async {
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
    .await;
    drop(shell);
    drop(pump);
    outcome
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
    // The gzip pump guard must outlive `cmd` (same contract as
    // `run_one_phase_bash`): declared BEFORE `cmd` so reverse drop
    // order destroys it AFTER `cmd` on every path; filled from the
    // sink below. The explicit `drop(cmd)` after the wait closes the
    // parent write-end copies deterministically.
    let mut pump_guard: Option<LogPump> = None;
    let mut cmd = sandbox_wrapped_command(&bin_dir.join("misc-functions.sh"), dyn_command, iso);
    // Same curated environment real `MiscFunctionsProcess` gets (the
    // phase env, not the inherited process env): `misc-functions.sh`
    // sources `bin/ebuild.sh`, whose `__preprocess_ebuild_env` *saves*
    // the live env back to `${T}/environment` when a binpkg's
    // `environment.raw` marker is present -- so an unfiltered inherited
    // env (the calling `cargo test`/shell process) would leak into the
    // regenerated vdb env. See `run_one_phase_bash`'s own `env_clear`
    // for the phase-side equivalent.
    cmd.env_clear();
    cmd.envs(std::env::vars().filter(|(k, _)| environ_whitelisted(k) && k != "LC_ALL"));
    cmd.envs(vars);
    if let Some(path) = log_file {
        let sink = open_log_file(path)?;
        cmd.stdout(sink.out).stderr(sink.err);
        pump_guard = sink.pump;
    }
    let status = spawn_trackable(&mut cmd)
        .map_err(|e| format!("spawning real bash for {dyn_command} failed: {e}"))?;
    drop(cmd);
    // Explicit (a pure-drop guard reads as unused otherwise): joins the
    // gzip thread now that every write end is closed.
    drop(pump_guard);
    Ok(status.code().unwrap_or(1))
}

/// One process-wide multi-threaded tokio runtime (real `_emerge/
/// Scheduler.py` runs its whole invocation -- every package's every
/// phase -- on a single `asyncio` event loop; portuale's own sync entry
/// points -- `run_misc_function`/`run_commands_logged`/
/// `run_single_phase`/`run_phase_from_saved_env` -- each used to spin up
/// and tear down their own fresh `Builder::new_multi_thread()` runtime,
/// meaning a single `emerge` merging many packages paid that thread-pool
/// setup/teardown cost once per phase per package, not once per
/// invocation). `OnceLock` rather than a value threaded through every
/// call site: none of these entry points are themselves called from
/// inside an existing tokio context (`ebuild_phases::run_one_phase`'s
/// own async body never calls back into any of the four), so lazily
/// building this once and reusing it from everywhere is safe, and
/// avoids turning this into a signature change touching every caller
/// across `ebuild.rs`/`ebuild_merge.rs`/`ebuild_package.rs`/
/// `emerge_build.rs`. Never torn down -- this is a CLI process, not a
/// long-lived server; process exit reclaims the thread pool same as any
/// other resource.
fn shared_runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    use std::sync::OnceLock;
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("failed to start async runtime: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
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
    let runtime = shared_runtime()?;
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
        let (a, _aa) = fetch_sources(
            &env,
            root,
            distdir,
            debug,
            config_root,
            shell,
            &features_string(&extra_env),
            // The resolved `USE` threaded by the caller (last `USE` pair,
            // the value the phases themselves see), so `A` names exactly
            // the distfiles `use()` will expect.
            extra_env
                .iter()
                .rev()
                .find(|(k, _)| k == "USE")
                .map_or("", |(_, v)| v.as_str()),
        )
        .await?;
        // Real `config.environ()` exports `A` but pops `AA` for every
        // EAPI >= 4 (`config.py:3331-3333`, `eapi_exports_AA`); the
        // EAPI floor here is 5+, so `AA` is never exported (S0 finding
        // `l2-env-aa-exported`).
        extra_env.push(("A".to_string(), a.join(" ")));
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
                // Real `EbuildPhase._ebuild_exit_unlocked` order
                // (`EbuildPhase.py:424-438`): `_post_src_install_write_
                // metadata` and `_post_src_install_uid_fix` run right
                // after `bin/ebuild.sh install` returns and **before**
                // the `install_qa_check` post-phase commands below. That
                // ordering is load-bearing for `SIZE`: it is the
                // *pre-transform* image (`ecompress`/`estrip` run later),
                // real's own 807 bytes for `porttest/docs` against the
                // 372 the compressed image would give.
                write_post_install_metadata(&env, root, build_env)?;
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
                // Real `EbuildPhase._commands_exit` runs
                // `_post_src_install_soname_symlinks` immediately after
                // that same misc-function sequence (see its own doc
                // comment): the 6-field `NEEDED.ELF.2` (from the
                // post-`estrip` image), generated `PROVIDES`/
                // `REQUIRES`, and `_inject_libc_dep`'s own implicit libc
                // dependency (#39).
                write_post_install_soname_deps(&env, root)?;
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
/// `/var/tmp`; the builddir adds `portage/<cat>/<pf>` on top) is read
/// by the caller, not internally here --
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
    let runtime = shared_runtime()?;
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

/// Opens `log_file` for append (creating it and its parent dir),
/// returning the two write ends a phase's stdout+stderr go to plus the
/// gzip pump guard when the path ends in `.gz`
/// (`FEATURES=compress-build-logs`, whose `.gz`-suffixed paths
/// `emerge_build::build_log_path` produces).
///
/// Real `EbuildPhase._open_log` (`_emerge/EbuildPhase.py`): the log
/// file is opened `ab` and, when its name ends `.gz`, wrapped in a
/// `gzip.GzipFile(mode="ab")` -- every phase appends one gzip member to
/// the same file, and the failure-tail / QA readers wrap it in
/// `gzip.GzipFile(mode="rb")` back. The pump thread below is that
/// wrapper: the phase (a real bash child, or the embedded brush shell
/// writing its fd table) writes plain bytes into two pipes, and the
/// thread gzip-encodes them into the `.gz` file -- one member per
/// `open_log_file` call, exactly like real's one `GzipFile` per
/// `_open_log` call. A phase that writes nothing appends no member at
/// all (real's header is deferred to the first write too).
///
/// Lifetime: the guard MUST drop after the phase's write ends close
/// (child waited + parent `Command` dropped, or brush `Shell`
/// dropped), otherwise its join hangs waiting for EOF. Declare the
/// sink early and `drop` the writer holder explicitly -- see the call
/// sites. Joining first also guarantees the `.gz` member is finished
/// before any reader (failure tail, next phase's append, QA scan)
/// touches the file.
struct LogSink {
    out: std::fs::File,
    err: std::fs::File,
    pump: Option<LogPump>,
}

struct LogPump {
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for LogPump {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One pipe's read end drained into the shared gzip encoder; returns
/// bytes forwarded. Both pipes' pumps share one `GzEncoder` (under a
/// mutex -- stdout/stderr chunk interleaving was never deterministic),
/// and the member is finished only when at least one byte flowed.
fn pump_one_pipe(
    mut read_end: std::fs::File,
    encoder: &std::sync::Arc<std::sync::Mutex<flate2::write::GzEncoder<std::fs::File>>>,
) -> u64 {
    use std::io::{Read, Write};
    let mut forwarded: u64 = 0;
    let mut buf = [0u8; 65536];
    loop {
        match read_end.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let Ok(mut enc) = encoder.lock() else {
                    break;
                };
                if enc.write_all(&buf[..n]).is_err() {
                    break;
                }
                forwarded += n as u64;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    forwarded
}

fn open_log_file(log_file: &Path) -> Result<LogSink, String> {
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let is_gz = log_file.extension().is_some_and(|ext| ext == "gz");
    if !is_gz {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)
            .map_err(|e| format!("{}: {e}", log_file.display()))?;
        let g = f
            .try_clone()
            .map_err(|e| format!("{}: {e}", log_file.display()))?;
        return Ok(LogSink {
            out: f,
            err: g,
            pump: None,
        });
    }
    let target = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)
        .map_err(|e| format!("{}: {e}", log_file.display()))?;
    let pipe = |what: &str| -> Result<(std::fs::File, std::fs::File), String> {
        let mut fds = [0; 2];
        // `O_CLOEXEC`: the pump's read ends must not leak into the
        // phase child across fork/exec (hygiene only -- EOF keys off
        // the write ends).
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(format!(
                "pipe for {what}: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(unsafe {
            (
                std::fs::File::from_raw_fd(fds[0]),
                std::fs::File::from_raw_fd(fds[1]),
            )
        })
    };
    let (out_r, out_w) = pipe("build-log stdout")?;
    let (err_r, err_w) = pipe("build-log stderr")?;
    let thread = std::thread::spawn(move || {
        let encoder = std::sync::Arc::new(std::sync::Mutex::new(flate2::write::GzEncoder::new(
            target,
            flate2::Compression::default(),
        )));
        let encoder_err = encoder.clone();
        let encoder_out = encoder.clone();
        let out_pump = std::thread::spawn(move || pump_one_pipe(out_r, &encoder_out));
        let err_bytes = pump_one_pipe(err_r, &encoder_err);
        let out_bytes = out_pump.join().unwrap_or(0);
        if out_bytes + err_bytes > 0
            && let Ok(mut enc) = encoder.lock()
        {
            // Trailer for this phase's member; the next phase
            // appends a fresh member (real's per-`_open_log`
            // `GzipFile` does the same).
            let _ = enc.try_finish();
            let _ = enc.get_mut().sync_all();
        }
        // Else: drop the encoder unfinished -- nothing was ever
        // written, so no header went out either, and the file is left
        // exactly as found (real's deferred header behaves the same).
    });
    Ok(LogSink {
        out: out_w,
        err: err_w,
        pump: Some(LogPump {
            thread: Some(thread),
        }),
    })
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
) -> Result<(brush_core::ExecutionParameters, Option<LogPump>), String> {
    let mut pump = None;
    if let Some(path) = log_file {
        let sink = open_log_file(path)?;
        shell
            .open_files_mut()
            .set_fd(brush_core::openfiles::OpenFiles::STDOUT_FD, sink.out.into());
        shell
            .open_files_mut()
            .set_fd(brush_core::openfiles::OpenFiles::STDERR_FD, sink.err.into());
        // The shell owns the pipe write ends now (its fd table); the
        // caller holds the guard across execution and drops the shell
        // first -- see `open_log_file`'s lifetime contract.
        pump = sink.pump;
    }
    Ok((shell.default_exec_params(), pump))
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
    // Real `Scheduler._background_mode`'s own `PORTAGE_LOG_FILE`,
    // extended to this hook -- see `ebuild_merge::MergeOptions::
    // log_file`'s own doc comment. `None` for a standalone/foreground
    // run.
    log_file: Option<&Path>,
) -> Result<i32, String> {
    let runtime = shared_runtime()?;
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
            log_file,
        )
        .await
    })
}

/// Real `clean` phase (`bin/ebuild.sh clean` -> `bin/phase-functions.sh:
/// 316`'s `__dyn_clean`): removes `${PORTAGE_BUILDDIR}/image`,
/// `.installed` and the `.pretended`/`.setuped`/`.unpacked`/`.compiled`/
/// `.installed` resume markers, plus `${T}` and `WORKDIR`/`build-info`/
/// `files` unless `FEATURES=keeptemp`/`keepwork` (backlog #42).
///
/// Real starts this phase from four places (all vendored, all read for
/// this grounding):
///   - `_emerge/EbuildBuild.py:207-229` (`_start_pre_clean`): after
///     locking the builddir and **before every build**, unconditionally
///     -- `noclean` does not skip it, only the phase's own
///     `keeptemp`/`keepwork` checks do. Without it, a stale `.installed`
///     marker makes `__dyn_install` print "already installed; skipping"
///     and repackage an already-`instprep`ped image (the #38 S4 repro).
///   - `_emerge/Scheduler.py:969-981` and `:1119-1139`: the same clean
///     before a pretend/build when an existing builddir is present.
///   - `_emerge/Binpkg.py:305`: before unpacking a binary package.
///   - `_emerge/EbuildBuild.py:525-535` (`_buildpkgonly_success_hook_exit`)
///     after a `--buildpkgonly` package, and `dbapi/vartree.py:6183-6198`
///     (`dblink.merge()`'s tail) after a merge unless `FEATURES=noclean`
///     -- the post-merge half of #42.
///
/// A thin wrapper over `run_single_phase` (real only ever starts the
/// phase through an `EbuildPhase`, i.e. `bin/ebuild.sh clean`), kept as
/// its own named entry point so every call site below reads as the real
/// mechanism.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_clean(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    build_env: &[(String, String)],
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
    log_file: Option<&Path>,
) -> Result<i32, String> {
    run_single_phase(
        ebuild_path,
        "clean",
        root,
        portage_tmpdir,
        debug,
        config_root,
        shell,
        build_env,
        log_file,
    )
}

/// Real `_emerge/BinpkgEnvExtractor`: `${T}/environment` <- the binpkg's
/// `environment.bz2`, plus the `${T}/environment.raw` marker (see
/// `run_phase_from_saved_env`).
fn seed_saved_environment(env: &Environment, saved_env_bz2: &Path) -> Result<(), String> {
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
        .map_err(|e| format!("{}: {e}", env.t().join("environment.raw").display()))
}

/// `MERGE_TYPE=binary` (real `_emerge/Binpkg.py:92` +
/// `doebuild.py:1288` for `tree == "bintree"`): a
/// `portage_readonly_vars` entry, so it's stripped from the saved
/// env and must be re-supplied here. Load-bearing for eclasses
/// that gate build-time work on it -- e.g.
/// `python-any-r1_pkg_setup` is `[[ ${MERGE_TYPE} != binary ]] &&
/// python_setup`, so without this a binpkg merge runs
/// `python_setup` -> `python_check_deps` against BDEPEND that was
/// never installed (a `--getbinpkg` binary needs no build deps)
/// and `die`s "No supported Python implementation installed".
/// `EMERGE_FROM` alone doesn't cover it -- the eclasses check
/// `MERGE_TYPE`.
fn binary_merge_env() -> Vec<(String, String)> {
    vec![
        ("EMERGE_FROM".to_string(), "binary".to_string()),
        ("MERGE_TYPE".to_string(), "binary".to_string()),
    ]
}

/// Real `dblink.treewalk()`'s first step (`vartree.py:4440-4450`,
/// `doebuild.py:880` `"instprep": {"cmd": misc_sh}`): the `instprep`
/// internal phase, `bin/misc-functions.sh __dyn_instprep`, on *every*
/// merge -- source or binary -- before `INSTALL_MASK`, collision-protect,
/// `pkg_preinst` or a single file is copied.
///
/// All transform logic stays in the vendored script (`misc-functions.sh:
/// 265-308`): it `ecompress`es iff `PORTAGE_COMPRESS` is set and
/// `binpkg-docompress` is absent, `estrip`s iff `binpkg-dostrip` is
/// absent, and is idempotent through `${PORTAGE_BUILDDIR}/.instprepped`.
/// With the default `FEATURES` both tokens are on, `install_qa_check`
/// already ran the transforms, and this is a near no-op; it is the only
/// place they happen under `FEATURES="-binpkg-dostrip
/// -binpkg-docompress"` (whose archives carry an unstripped image).
///
/// `saved_env_bz2`: `Some` for a binary merge -- `${T}/environment` is
/// seeded from the binpkg's saved env and `EMERGE_FROM`/`MERGE_TYPE=
/// binary` are set (real `Binpkg` + `BinpkgEnvExtractor`); `None` for a
/// source merge, whose `${T}/environment` is still the install chain's.
/// `build_env` is the resolved phase env (`FEATURES`,
/// `PORTAGE_COMPRESS*`, ...) the gates read.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_instprep(
    ebuild_path: &Path,
    saved_env_bz2: Option<&Path>,
    // Extract `${T}/environment` from `saved_env_bz2` first: `true` when
    // this is the first phase of a binary merge (no earlier hook has
    // seeded it), `false` when `pkg_setup` already ran -- real extracts
    // the archive env **once** and every phase evolves it, so a second
    // extraction here would wipe `pkg_setup`'s own variables (e.g.
    // `linux-info`'s `SKIP_KERNEL_BINPKG_ENV_RESET`).
    seed: bool,
    root: &Path,
    portage_tmpdir: &Path,
    build_env: &[(String, String)],
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
    log_file: Option<&Path>,
) -> Result<i32, String> {
    let runtime = shared_runtime()?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        let mut extra_env = build_env.to_vec();
        if let Some(saved) = saved_env_bz2 {
            if seed {
                seed_saved_environment(&env, saved)?;
            }
            extra_env.extend(binary_merge_env());
        }
        run_misc_functions(
            &env,
            root,
            "instprep",
            "__dyn_instprep",
            &extra_env,
            debug,
            config_root,
            shell,
            log_file,
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
///
/// `seed` controls the `${T}/environment` extraction: real
/// `_emerge/BinpkgEnvExtractor` runs **once per package** and every
/// subsequent phase (`pkg_setup`, `pkg_preinst`, `pkg_postinst`, …)
/// evolves that one file (`PORTAGE_UPDATE_ENV` re-saves it at the end
/// of the hook phases). A caller that runs several hooks against the
/// same builddir must therefore pass `false` after the first one: a
/// per-hook re-seed would wipe every variable a previous phase set
/// (`linux-info`'s `SKIP_KERNEL_BINPKG_ENV_RESET`, the acct-user
/// eclass's own `_ACCT_USER_*`) before the vdb env is regenerated.
/// `unmerge` hooks use `true` (each old version seeds its own
/// builddir).
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_phase_from_saved_env(
    ebuild_path: &Path,
    saved_env_bz2: &Path,
    // Extract `${T}/environment` from `saved_env_bz2` first (see the
    // doc comment above): `true` for the first hook of a package /
    // an unmerge, `false` for the following hooks of the same merge.
    seed: bool,
    phase: &str,
    root: &Path,
    portage_tmpdir: &Path,
    debug: bool,
    config_root: &Path,
    shell: ShellBackend,
    // Real `Scheduler._background_mode`'s own `PORTAGE_LOG_FILE`,
    // extended to this hook -- see `ebuild_merge::MergeOptions::
    // log_file`'s own doc comment. `None` for a standalone/foreground
    // run.
    log_file: Option<&Path>,
    // Real `dblink.treewalk`'s `PORTAGE_UPDATE_ENV` (`vartree.py:5334`,
    // set around the merge's `postinst` phase to
    // `<dbpkgdir>/environment.bz2`): `bin/phase-functions.sh`'s
    // `prerm|postrm|preinst|postinst|config|info` case regenerates that
    // file from the *live* phase environment
    // (`___save_and_filter_ebuild_env … | bzip2 -9 > $PORTAGE_UPDATE_ENV`)
    // -- so the vdb env carries the merge-time config, not the binpkg's
    // build-time one, and stale locals a different build host baked in
    // are filtered out. `None` for every hook that must not touch the
    // vdb env. `refresh_features`, when `Some` and non-empty, overrides
    // `FEATURES`/`PORTAGE_FEATURES` in the phase env for exactly that
    // regeneration -- real portage's vdb env carries the resolved
    // incremental list.
    update_env: Option<&Path>,
    refresh_features: Option<&str>,
) -> Result<i32, String> {
    let runtime = shared_runtime()?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        if seed {
            seed_saved_environment(&env, saved_env_bz2)?;
        }

        let mut extra_env = binary_merge_env();
        if let Some(p) = update_env {
            extra_env.push(("PORTAGE_UPDATE_ENV".to_string(), p.display().to_string()));
            if let Some(features) = refresh_features.filter(|f| !f.is_empty()) {
                extra_env.push(("FEATURES".to_string(), features.to_string()));
                extra_env.push(("PORTAGE_FEATURES".to_string(), features.to_string()));
            }
        }
        run_one_phase(
            &env,
            root,
            phase,
            debug,
            &extra_env,
            config_root,
            shell,
            log_file,
        )
        .await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_bin_overlay_dir_is_idempotent() {
        // Backlog #88: the `atexit` handler's contract -- removes a
        // populated overlay, then silently tolerates the missing dir
        // (double exit paths, or an overlay a previous run already
        // reclaimed, must never fail).
        let dir =
            std::env::temp_dir().join(format!("portuale-bin-overlay-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("f"), "x").unwrap();
        remove_bin_overlay_dir(&dir);
        assert!(!dir.exists());
        remove_bin_overlay_dir(&dir);
        assert!(!dir.exists());
    }

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
    fn sandbox_wrapped_command_configures_the_real_addrconfig_workaround_addresses() {
        // Real `_configure_loopback_interface` (`process.py:838-871`,
        // bug #690758): `ip link set lo up` plus a real, non-loopback-
        // looking address per family, so glibc's `getaddrinfo()` with
        // `AI_ADDRCONFIG` set doesn't return spuriously empty results
        // for a build that only ever talks to `localhost`.
        let iso = Isolation {
            net: true,
            ipc: false,
            mount: false,
            pid: false,
            fs_sandbox: false,
        };
        let cmd = sandbox_wrapped_command(Path::new("/bin/true"), "install", iso);
        let shim = cmd
            .get_args()
            .filter_map(|a| a.to_str())
            .find(|a| a.contains("exec"))
            .expect("the sh -c shim argument is present");
        assert!(shim.contains("ip link set lo up"));
        assert!(shim.contains("ip addr add 10.0.0.1/8 dev lo"));
        assert!(shim.contains("ip -6 addr add fd::1/8 dev lo"));
    }

    #[test]
    fn features_test_passthrough_actually_runs_src_test() {
        // A real subprocess invocation, not `std::env::set_var` --
        // `run_commands`'s own doc comment already establishes that
        // mutating process-global env vars is unsound from parallel
        // test threads. Spawning the real `portuale` binary gives each
        // invocation its own independent environment, exactly the way
        // real portage's own `FEATURES` behavior is env-var-scoped.
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-features_test_passthrough",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("pkg/dev-libs/srctestpkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        let ebuild = pkg_dir.join("srctestpkg-1.0.ebuild");
        std::fs::write(
            &ebuild,
            "EAPI=8\nSLOT=\"0\"\nsrc_test() { touch \"${T}/test-ran\" || die; }\n",
        )
        .unwrap();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        let marker = portage_tmpdir.join("portage/dev-libs/srctestpkg-1.0/temp/test-ran");

        // `CARGO_BIN_EXE_<name>` is only set for `tests/` integration
        // targets, not for a unit test compiled into the crate's own
        // binary -- derive the sibling `portuale` binary's path from
        // this very test binary's own path instead (`target/<profile>/
        // deps/portuale-<hash>` -> `target/<profile>/portuale`).
        let mut portuale_bin = std::env::current_exe().expect("current test exe");
        portuale_bin.pop();
        if portuale_bin.ends_with("deps") {
            portuale_bin.pop();
        }
        portuale_bin.push("portuale");

        let run = |features: &str| {
            let _ = std::fs::remove_file(&marker);
            let status = std::process::Command::new(&portuale_bin)
                .env_clear()
                .env("PATH", std::env::var("PATH").unwrap())
                .env("HOME", std::env::var("HOME").unwrap_or_default())
                .env("ROOT", &root)
                .env("PORTAGE_TMPDIR", &portage_tmpdir)
                .env("FEATURES", features)
                .args(["ebuild", ebuild.to_str().unwrap(), "test"])
                .status()
                .expect("portuale ebuild spawns");
            assert!(status.success(), "FEATURES={features:?}: {status:?}");
        };

        // Real `__dyn_test` (`phase-functions.sh:559`): `src_test` is a
        // no-op ("Test phase [not enabled]") unless `FEATURES` contains
        // `test` -- previously always the case here regardless of the
        // real, outer `FEATURES` the invoker actually asked for, since
        // the phase's own exported `FEATURES` was unconditionally
        // blanked (`phase_features_value`'s own doc comment).
        run("");
        assert!(!marker.exists(), "src_test ran without FEATURES=test");

        run("test");
        assert!(marker.exists(), "src_test did not run with FEATURES=test");
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
        let (restrict, properties) =
            restrict_and_properties(&env, &std::collections::HashSet::new());
        assert_eq!(restrict, "");
        assert_eq!(properties, "live");
    }

    #[test]
    fn depend_phase_reduces_restrict_on_the_config_use_set() {
        // A conditional `RESTRICT` drops its gated tokens on the empty
        // set but keeps them on the config `USE` set -- the `depend`
        // phase's own reduction (real `doebuild(mydo="depend")` runs
        // with the `setcpv` config `USE`, so ebuild.sh's own
        // `contains_word … "${PORTAGE_RESTRICT}"` checks, e.g. the
        // `strip`/`nostrip` `DEBUGBUILD` gate, see the configured
        // tokens). No fixture change: a throwaway repo carries the
        // conditional entry, while the config `USE` comes from the
        // fixture tree (`make.conf` sets `confflag`).
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-depend_use_set",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = tmp.join("repo");
        std::fs::create_dir_all(repo.join("profiles")).unwrap();
        std::fs::write(repo.join("profiles/repo_name"), "testrepo\n").unwrap();
        let pkg_dir = repo.join("dev-libs/condrestrictpkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("condrestrictpkg-1.0.ebuild"),
            "EAPI=8\nSLOT=\"0\"\n",
        )
        .unwrap();
        let cache_dir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(
            cache_dir.join("condrestrictpkg-1.0"),
            "DEFINED_PHASES=-\nEAPI=8\nIUSE=\nKEYWORDS=amd64\n\
             RESTRICT=confflag? ( strip ) other? ( bindist )\nSLOT=0\n\
             _md5_=0000000000000000000000000000000\n",
        )
        .unwrap();
        let portage_tmpdir = tmp.join("pt");
        let env = compute_environment(&pkg_dir.join("condrestrictpkg-1.0.ebuild"), &portage_tmpdir)
            .expect("synthetic ebuild parses");
        let empty = std::collections::HashSet::new();
        assert_eq!(
            restrict_and_properties(&env, &empty),
            (String::new(), String::new())
        );
        // The fixture config's own `USE` carries `confflag` (its
        // `make.conf`), so the `depend` reduction keeps `strip` while
        // still dropping the `other?` group.
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let use_set = depend_use_set(&env, &fixtures, &fixtures);
        assert!(use_set.contains("confflag"), "{use_set:?}");
        assert_eq!(
            restrict_and_properties(&env, &use_set),
            ("strip".to_string(), String::new())
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn flat_field_on_dedups_and_sorts_like_real_flatten() {
        // Real `config.py::_flatten` (`:1681-1688`):
        // `" ".join(sorted(set(use_reduce(..., flat=True))))` -- the
        // #45 ncurses case: `RESTRICT="!test? ( test ) test"` reduces
        // to two `test` tokens with `test` off, and the set collapses
        // them (real's saved value is `"test"`).
        let empty = std::collections::HashSet::new();
        assert_eq!(flat_field_on("!test? ( test ) test", &empty), "test");
        assert_eq!(flat_field_on("zoo alpha zoo", &empty), "alpha zoo");
        let mut use_set = std::collections::HashSet::new();
        use_set.insert("bar".to_string());
        assert_eq!(
            flat_field_on("foo bar? ( bar baz )", &use_set),
            "bar baz foo"
        );
        assert_eq!(flat_field_on("foo bar? ( bar baz )", &empty), "foo");
        assert_eq!(flat_field_on("", &empty), "");
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
            restrict_and_properties(&env, &std::collections::HashSet::new()),
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

    /// Backlog #47: real `_post_src_install_write_metadata`
    /// (`doebuild.py:2727-2732`) writes `build-info/BUILD_TIME` (epoch
    /// seconds) before any metadata key, and the vdb copy +
    /// `_consolidate_to_metadata_file` carry it. The install chain must
    /// leave that file behind for the merge to copy.
    #[test]
    fn install_writes_the_real_build_time_file() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "install_writes_the_real_build_time_file"
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

        let build_time = portage_tmpdir.join("portage/dev-libs/phasepkg-1.0/build-info/BUILD_TIME");
        let text = std::fs::read_to_string(&build_time)
            .unwrap_or_else(|e| panic!("{} should exist: {e}", build_time.display()));
        assert!(
            text.trim().parse::<u64>().is_ok(),
            "BUILD_TIME must be an epoch-seconds integer: {text:?}"
        );

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }

    /// Backlog #42: the real `clean` phase (`bin/ebuild.sh clean` ->
    /// `__dyn_clean`) is what real `_emerge/EbuildBuild._start_pre_clean`
    /// runs before every build. Given a stale `.installed` marker and a
    /// populated `image/` + `${T}`, it must remove all three -- exactly
    /// the state that made a rebuild silently skip `install` and
    /// repackage an already-`instprep`ped image (#38 S4).
    #[test]
    fn run_clean_removes_a_stale_installed_marker_image_and_temp() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-run-clean",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);
        let builddir = portage_tmpdir.join("portage/dev-libs/phasepkg-1.0");
        std::fs::create_dir_all(builddir.join("image/usr/share/phasepkg")).unwrap();
        std::fs::write(
            builddir.join("image/usr/share/phasepkg/hello.txt"),
            "stale\n",
        )
        .unwrap();
        std::fs::write(builddir.join(".installed"), []).unwrap();
        std::fs::create_dir_all(builddir.join("temp")).unwrap();
        std::fs::write(builddir.join("temp/build.log"), "stale log\n").unwrap();

        let status = run_clean(
            &ebuild_path,
            Path::new("/"),
            &portage_tmpdir,
            &[],
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Bash,
            None,
        )
        .expect("run_clean should not itself error");
        assert_eq!(status, 0, "clean should exit successfully");
        assert!(
            !builddir.join(".installed").exists(),
            "a stale .installed marker must be removed"
        );
        assert!(
            !builddir.join("image").exists(),
            "a stale image must be removed"
        );
        assert!(
            !builddir.join("temp").exists(),
            "T must be removed without keeptemp/keepwork"
        );

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }

    /// `FEATURES=keepwork` (real `__dyn_clean`'s own guard): `clean`
    /// still drops the stale `.installed`/`image`, but keeps `${T}` and
    /// `WORKDIR` -- the documented real way to inspect a build.
    #[test]
    fn run_clean_with_keepwork_keeps_temp_and_workdir_but_still_drops_the_marker() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-run-clean-keepwork",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);
        let builddir = portage_tmpdir.join("portage/dev-libs/phasepkg-1.0");
        std::fs::create_dir_all(builddir.join("image/usr/share/phasepkg")).unwrap();
        std::fs::create_dir_all(builddir.join("temp")).unwrap();
        std::fs::write(builddir.join("temp/build.log"), "kept\n").unwrap();
        std::fs::write(builddir.join(".installed"), []).unwrap();

        let keepwork = vec![("FEATURES".to_string(), "keepwork".to_string())];
        let status = run_clean(
            &ebuild_path,
            Path::new("/"),
            &portage_tmpdir,
            &keepwork,
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Bash,
            None,
        )
        .expect("run_clean should not itself error");
        assert_eq!(status, 0, "clean should exit successfully");
        assert!(!builddir.join(".installed").exists());
        assert!(!builddir.join("image").exists());
        assert!(
            builddir.join("temp/build.log").is_file(),
            "keepwork must keep T"
        );

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }

    /// Real, end-to-end proof of the `binpkg-docompress` transform path
    /// (#38 S1): with `PORTAGE_COMPRESS=bzip2` and `FEATURES=binpkg-
    /// docompress` in the phase env, real, unmodified `install_qa_check`
    /// (`misc-functions.sh:147-152`) runs real `bin/ecompress` over
    /// `${D}` -- the >128 B doc (the `PORTAGE_DOCOMPRESS_SIZE_LIMIT`)
    /// becomes `BIG.txt.bz2`, the smaller one is untouched, and the
    /// dangling link into the now-compressed doc is repaired (and
    /// suffixed) by `ecompress`'s own `fix_symlinks`. Uses the default
    /// `bash` backend (`ShellBackend::Bash`), the same one a live
    /// `emerge` runs.
    #[test]
    fn install_qa_check_docompress_compresses_docs_and_repairs_symlinks() {
        if std::process::Command::new("bzip2")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: bzip2 not available on this host");
            return;
        }
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/doccompresspkg/doccompresspkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-docompress",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);

        let extra_env = vec![
            ("PORTAGE_COMPRESS".to_string(), "bzip2".to_string()),
            ("FEATURES".to_string(), "binpkg-docompress".to_string()),
        ];
        let status = run_commands(
            &ebuild_path,
            &["install"],
            Path::new("/"),
            &portage_tmpdir,
            &portage_tmpdir.join("distfiles"),
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Bash,
            &extra_env,
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should exit successfully");

        let docdir = portage_tmpdir
            .join("portage/dev-libs/doccompresspkg-1.0/image/usr/share/doc/doccompresspkg-1.0");
        assert!(
            docdir.join("BIG.txt.bz2").is_file(),
            "a doc above PORTAGE_DOCOMPRESS_SIZE_LIMIT must be compressed"
        );
        assert!(
            !docdir.join("BIG.txt").exists(),
            "ecompress must remove the uncompressed original"
        );
        assert!(
            docdir.join("small.txt").is_file(),
            "a doc below the size limit must stay uncompressed"
        );
        assert_eq!(
            std::fs::read_link(docdir.join("link-to-big.txt.bz2")).unwrap(),
            Path::new("BIG.txt.bz2"),
            "a symlink into a compressed doc must be repaired and suffixed"
        );

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }

    /// #38 S4: the merge-time `instprep` phase (`misc-functions.sh:
    /// 265-308`) is the complement of the `install_qa_check` gate. With
    /// `binpkg-docompress` absent the install chain leaves `BIG.txt`
    /// plain; `run_instprep` with the token present is a no-op (real's
    /// `! contains_word` gate), and without it compresses the doc and
    /// repairs the link, then marks `.instprepped` so a re-run skips.
    #[test]
    fn run_instprep_applies_only_the_complement_of_the_install_qa_gate() {
        if std::process::Command::new("bzip2")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: bzip2 not available on this host");
            return;
        }
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/doccompresspkg/doccompresspkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-instprep",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);
        let no_root = Path::new("/dev/null/no-config-root");
        let env_with = |features: &str| {
            vec![
                ("PORTAGE_COMPRESS".to_string(), "bzip2".to_string()),
                // `binpkg-dostrip` stays on: this test is about compression.
                ("FEATURES".to_string(), format!("binpkg-dostrip {features}")),
            ]
        };

        let status = run_commands(
            &ebuild_path,
            &["install"],
            Path::new("/"),
            &portage_tmpdir,
            &portage_tmpdir.join("distfiles"),
            false,
            no_root,
            ShellBackend::Bash,
            &env_with("-binpkg-docompress"),
        )
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should exit successfully");
        let builddir = portage_tmpdir.join("portage/dev-libs/doccompresspkg-1.0");
        let docdir = builddir.join("image/usr/share/doc/doccompresspkg-1.0");
        assert!(
            docdir.join("BIG.txt").is_file(),
            "install_qa_check must not compress without binpkg-docompress"
        );

        let instprep = |features: &str| {
            run_instprep(
                &ebuild_path,
                None,
                false,
                Path::new("/"),
                &portage_tmpdir,
                &env_with(features),
                false,
                no_root,
                ShellBackend::Bash,
                None,
            )
            .expect("run_instprep should not itself error")
        };

        assert_eq!(instprep("binpkg-docompress"), 0);
        assert!(
            docdir.join("BIG.txt").is_file(),
            "instprep must not compress when binpkg-docompress is on"
        );
        assert!(builddir.join(".instprepped").is_file());

        std::fs::remove_file(builddir.join(".instprepped")).unwrap();
        assert_eq!(instprep("-binpkg-docompress"), 0);
        assert!(docdir.join("BIG.txt.bz2").is_file());
        assert!(!docdir.join("BIG.txt").exists());
        assert!(docdir.join("small.txt").is_file());
        assert_eq!(
            std::fs::read_link(docdir.join("link-to-big.txt.bz2")).unwrap(),
            Path::new("BIG.txt.bz2")
        );
        assert!(builddir.join(".instprepped").is_file());

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
            None,
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
            None,
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
            None,
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
            None,
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
    fn install_computes_real_a_from_a_verified_distfile_and_leaves_aa_unset() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/verifiedfetchpkg/verifiedfetchpkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "install_computes_real_a_from_a_verified_distfile_and_leaves_aa_unset"
        ));
        let distdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-distdir-{}-{}",
            std::process::id(),
            "install_computes_real_a_from_a_verified_distfile_and_leaves_aa_unset"
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
        // `AA` is unset: real `config.environ()` pops it for every
        // EAPI >= 4 (`config.py:3331-3333`); the EAPI floor here is 5+.
        // The ebuild's own `echo "AA=${AA}"` therefore writes an empty
        // value, matching a real EAPI-8 phase.
        assert_eq!(observed, "A=verifiedfetchpkg-1.0.tar.gz\nAA=\n");

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
        let _ = std::fs::remove_dir_all(&distdir);
    }

    /// #37 S2 step 6: the `depend` phase (real `doebuild(mydo="depend")`,
    /// the `--regen`/metadata path) never gets the standalone base env --
    /// `phase_standalone_base_env` short-circuits before any config load,
    /// so S2's resolved-config threading cannot leak into it. A real repo
    /// checkout with a resolvable package is used precisely so removing
    /// the guard would fail this test rather than silently pass.
    #[test]
    fn depend_phase_standalone_base_env_stays_empty_with_a_real_repo() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/phaseenvpkg/phaseenvpkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-depend-base-{}",
            std::process::id()
        ));
        let env = compute_environment(&ebuild_path, &portage_tmpdir).unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let (use_str, flags) =
            phase_standalone_base_env(&env, &fixtures, Path::new("/"), "depend", &[]);
        assert_eq!(use_str, "", "depend must not get a config-derived USE");
        assert!(flags.is_empty(), "depend must not get config build vars");
        // The `extra_env`-carries-USE guard is the same short-circuit.
        let (use_str, flags) = phase_standalone_base_env(
            &env,
            &fixtures,
            Path::new("/"),
            "install",
            &[("USE".to_string(), "confflag".to_string())],
        );
        assert_eq!(use_str, "");
        assert!(flags.is_empty());
    }

    /// #37 S3: the phase-execution gates read the **resolved** `FEATURES`
    /// threaded on `extra_env` (`features_string`), never the raw process
    /// env -- a `make.conf` `-sandbox` disables the sandbox even when the
    /// calling env exports `sandbox`. The helpers take the features string
    /// as a parameter precisely so this is directly testable.
    #[test]
    fn feature_gates_use_the_resolved_features_string() {
        assert!(!feature_token_present("buildpkg", "sandbox"));
        assert!(feature_token_present("sandbox usersandbox", "sandbox"));
        assert!(!network_sandbox_requested("distlocks"));
        assert!(network_sandbox_requested("network-sandbox"));
        assert!(!fs_sandbox_requested(""));
        assert!(fs_sandbox_requested("usersandbox"));
        // The extra_env pair wins over the process env (here: whatever the
        // test runner exported); an absent pair falls back to it.
        assert_eq!(
            features_string(&[("FEATURES".to_string(), "".to_string())]),
            ""
        );
        assert_eq!(
            features_string(&[
                ("FEATURES".to_string(), "raw".to_string()),
                ("FEATURES".to_string(), "resolved".to_string()),
            ]),
            "resolved"
        );
    }

    /// #37 S4 (L2 real set, pv/htop `KERNEL_DIR="//usr/src/linux"`): path
    /// variables lose their trailing slash from EAPI 7 on, so `ROOT=/`
    /// is exported empty.
    #[test]
    fn eapi_path_var_strips_the_trailing_slash_from_eapi_7() {
        assert_eq!(eapi_path_var("8", "//"), "");
        assert_eq!(eapi_path_var("7", "/var/tmp/p/image/"), "/var/tmp/p/image");
        assert_eq!(eapi_path_var("6", "/"), "/");
        assert_eq!(eapi_path_var("5", "/var/tmp/p/image/"), "/var/tmp/p/image/");
    }

    /// #37 S4: a threaded `PATH` pair (the resolved env.d `PATH`) is the
    /// base behind the helper dir, deduplicated like real `_doebuild_path`,
    /// and never replaces the helper-prefixed value verbatim.
    #[test]
    fn phase_path_prefixes_helpers_onto_the_threaded_path() {
        let helpers = Path::new("/bin/ebuild-helpers");
        let extra = [
            ("PATH".to_string(), "/env/bin".to_string()),
            (
                "PATH".to_string(),
                "/usr/local/bin:/bin/ebuild-helpers:/usr/bin:/usr/local/bin".to_string(),
            ),
        ];
        assert_eq!(
            phase_path(helpers, &extra),
            "/bin/ebuild-helpers:/usr/local/bin:/usr/bin"
        );
    }

    /// #37 S2 G6: every brush `export` value is single-quoted, so a
    /// config-derived value containing `$`, backticks, `\`, `"` or
    /// whitespace can neither word-split nor command-substitute. Before
    /// this gate the resolved config env reached `phase_setup_script`
    /// through Rust's `{:?}` Debug formatting, which does not protect
    /// against expansion.
    #[test]
    fn shell_single_quote_neutralises_every_shell_metacharacter() {
        assert_eq!(shell_single_quote("plain"), "'plain'");
        assert_eq!(shell_single_quote("a b"), "'a b'");
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
        assert_eq!(
            shell_single_quote("$(touch /tmp/pwned) `id` \\ \" $HOME"),
            "'$(touch /tmp/pwned) `id` \\ \" $HOME'"
        );
        assert_eq!(shell_single_quote(""), "''");
        // A `CFLAGS`-shaped value with a single quote and a newline.
        assert_eq!(
            shell_single_quote("-O2 -DMSG='hi'\n-Wl,-z,now"),
            "'-O2 -DMSG='\\''hi'\\''\n-Wl,-z,now'"
        );
    }

    /// #37 S2 step 2: `extra_env` carries the resolved `FEATURES` and is
    /// appended last, so it must win over `phase_env_vars`' own
    /// `phase_features_value()` base (the raw process env). Verified as
    /// brush *script composition* here; the real-phase execution proof
    /// is `emerge_build::tests::source_merge_with_resolved_config_
    /// threads_use_and_slot_into_the_phase` (which asserts the
    /// `FEATURES=` marker under both backends).
    #[test]
    fn phase_setup_script_exports_extra_env_features_last() {
        let ebuild = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/phaseenvpkg/phaseenvpkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-setup-script-{}",
            std::process::id()
        ));
        let env = compute_environment(&ebuild, &portage_tmpdir).unwrap();
        let script = phase_setup_script(
            &env,
            Path::new("/"),
            "install",
            false,
            bin_dir(),
            &bin_dir().join("ebuild-helpers"),
            Path::new("/dev/null/no-config-root"),
            &[("FEATURES".to_string(), "resolved one".to_string())],
        );
        let features: Vec<&str> = script
            .lines()
            .filter(|l| l.starts_with("export FEATURES="))
            .collect();
        assert!(!features.is_empty(), "no FEATURES export in script");
        assert_eq!(
            *features.last().unwrap(),
            "export FEATURES='resolved one'",
            "the extra_env FEATURES must be exported last to win"
        );
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

    /// B2: a saved `${T}/environment` that does not parse must fail the
    /// next phase loudly -- real `bin/ebuild.sh:580`'s own `source
    /// "${T}"/environment || die "error sourcing environment"` -- never
    /// silently continue and run the `default` phase function against a
    /// half-loaded environment (the #38 G3 smoke's empty image with rc 0).
    /// `run_single_phase` runs exactly one phase, with no `actionmap_deps`
    /// chain that would re-save a fresh environment first: the same shape
    /// a real builddir resume has when portage starts the next phase.
    #[test]
    fn a_corrupt_saved_environment_fails_the_next_phase_in_both_backends() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/heredocpkg/heredocpkg-1.0.ebuild");
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "a_corrupt_saved_environment_fails_the_next_phase_in_both_backends"
        ));
        let _ = std::fs::remove_dir_all(&tmp);

        for (shell, name) in [(ShellBackend::Bash, "bash"), (ShellBackend::Brush, "brush")] {
            let portage_tmpdir = tmp.join(name);
            let unpack_status = run_commands(
                &ebuild_path,
                &["unpack"],
                Path::new("/"),
                &portage_tmpdir,
                &portage_tmpdir.join("distfiles"),
                false,
                Path::new("/dev/null/no-config-root"),
                shell,
                &[],
            )
            .expect("run_commands should not itself error");
            assert_eq!(unpack_status, 0, "{name}: unpack should exit successfully");

            // The post-phase save writes `${T}/environment`; break it with
            // a genuine syntax error (an unterminated quote -- an
            // unterminated here-document is only a warning to bash, not a
            // parse error, so it would not fail the bash control).
            let environment =
                portage_tmpdir.join("portage/dev-libs/heredocpkg-1.0/temp/environment");
            assert!(
                environment.is_file(),
                "{name}: the unpack phase must save {environment:?}"
            );
            std::fs::write(&environment, "f() { echo \"unterminated\n").unwrap();

            let log = tmp.join(format!("{name}.log"));
            let compile_status = run_single_phase(
                &ebuild_path,
                "compile",
                Path::new("/"),
                &portage_tmpdir,
                false,
                Path::new("/dev/null/no-config-root"),
                shell,
                &[],
                Some(&log),
            )
            .expect("run_single_phase should not itself error");
            assert_ne!(
                compile_status, 0,
                "{name}: a corrupt saved environment must fail the phase"
            );
            let logged = std::fs::read_to_string(&log)
                .unwrap_or_else(|e| panic!("{} should have been written: {e}", log.display()));
            assert!(
                logged.contains("error sourcing environment"),
                "{name}: expected real ebuild.sh's own die in the phase log, got:\n{logged}"
            );
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// B3: real `bin/phase-functions.sh`'s `__filter_readonly_variables`
    /// lists bash's special variables by running `env -i -- "${BASH}" -c
    /// …`, so the embedded brush shell must carry a real `$BASH` -- and
    /// brush's own brace expansion must produce that list's fields even
    /// under the function's own `local IFS` (the two brush-side fixes are
    /// both `brush-pin.md`'s tracked work). When either is missing, the
    /// saved `${T}/environment` carries `BASHOPTS`/`EUID`/`PPID`/
    /// `SHELLOPTS`/`UID`, and re-sourcing it prints `cannot mutate
    /// readonly variable` (plus `env: ''` when `$BASH` itself is empty).
    #[test]
    fn brush_phase_env_is_filtered_of_bash_special_variables() {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-{}",
            std::process::id(),
            "brush_phase_env_is_filtered_of_bash_special_variables"
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);
        let log = portage_tmpdir.join("brush.log");

        let status = run_commands_logged(
            &ebuild_path,
            &["install"],
            Path::new("/"),
            &portage_tmpdir,
            &portage_tmpdir.join("distfiles"),
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Brush,
            Some(&log),
            &[],
        )
        .expect("run_commands_logged should not itself error");
        assert_eq!(status, 0, "install should exit successfully");

        let environment = portage_tmpdir.join("portage/dev-libs/phasepkg-1.0/temp/environment");
        let saved = std::fs::read_to_string(&environment)
            .unwrap_or_else(|e| panic!("{} should have been written: {e}", environment.display()));
        for name in ["BASHOPTS", "EUID", "PPID", "SHELLOPTS", "UID"] {
            assert!(
                !saved
                    .lines()
                    .any(|line| line.starts_with("declare") && line.contains(&format!(" {name}="))),
                "{name} must be filtered out of the saved environment"
            );
        }

        let logged = std::fs::read_to_string(&log)
            .unwrap_or_else(|e| panic!("{} should have been written: {e}", log.display()));
        assert!(
            !logged.contains("cannot mutate readonly variable"),
            "the phase log must not contain readonly-variable noise:\n{logged}"
        );
        assert!(
            !logged.contains("env: ''"),
            "the phase log must not contain an empty-$BASH `env` failure:\n{logged}"
        );

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
    fn compute_environment_links_builddir_files_to_the_repo_filesdir() {
        // Real `_prepare_fake_filesdir`: FILESDIR is builddir/files, a
        // symlink to the ebuild's own repo `files/`. Without it every
        // ebuild patch via `eapply` dies (L2 S5).
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-filesdir",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("repo/dev-libs/patchpkg");
        std::fs::create_dir_all(pkg_dir.join("files")).unwrap();
        std::fs::write(pkg_dir.join("patchpkg-1.0.ebuild"), "EAPI=8\nSLOT=0\n").unwrap();
        std::fs::write(pkg_dir.join("files/fix.patch"), "diff\n").unwrap();
        let portage_tmp = tmp.join("ptmp");
        let env = compute_environment(&pkg_dir.join("patchpkg-1.0.ebuild"), &portage_tmp).unwrap();
        let expected = pkg_dir.join("files");
        assert_eq!(std::fs::read_link(env.filesdir()).unwrap(), expected);
        assert!(env.filesdir().join("fix.patch").is_file());

        // Re-running must be idempotent (real unlinks a stale target).
        let env2 = compute_environment(&pkg_dir.join("patchpkg-1.0.ebuild"), &portage_tmp).unwrap();
        assert_eq!(std::fs::read_link(env2.filesdir()).unwrap(), expected);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn compute_environment_links_builddir_files_even_without_a_repo_filesdir() {
        // Backlog #94: real `_prepare_fake_filesdir` links
        // unconditionally -- a missing repo `files/` must yield a
        // dangling symlink, never a plain directory (whose `rm -f` in
        // `__dyn_clean` fails with "Is a directory" and aborts the
        // rest of the cleanup under `set -e`, leaving `/var/tmp`
        // residue -- host `emerge -1 sys-libs/timezone-data`).
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-filesdir-missing",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("repo/sys-libs/notimezonefiles");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("notimezonefiles-1.0.ebuild"),
            "EAPI=8\nSLOT=0\n",
        )
        .unwrap();
        assert!(!pkg_dir.join("files").exists());
        let portage_tmp = tmp.join("ptmp");
        let env =
            compute_environment(&pkg_dir.join("notimezonefiles-1.0.ebuild"), &portage_tmp).unwrap();
        assert_eq!(
            std::fs::read_link(env.filesdir()).unwrap(),
            pkg_dir.join("files")
        );
        assert!(
            std::fs::symlink_metadata(env.filesdir())
                .unwrap()
                .file_type()
                .is_symlink()
        );
        // ... and `create_directories` must leave the dangling link
        // alone instead of failing on it (or materialising the target).
        create_directories(&env).unwrap();
        assert!(
            std::fs::symlink_metadata(env.filesdir())
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!pkg_dir.join("files").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn compute_environment_replaces_a_stale_plain_filesdir_with_the_link() {
        // Migration path for pre-fix residue: a plain directory left at
        // builddir/files (the shape `__dyn_clean` choked on) becomes the
        // symlink again instead of erroring.
        let tmp = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-filesdir-stale-dir",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("repo/dev-libs/dirpkg");
        std::fs::create_dir_all(pkg_dir.join("files")).unwrap();
        std::fs::write(pkg_dir.join("dirpkg-1.0.ebuild"), "EAPI=8\nSLOT=0\n").unwrap();
        let portage_tmp = tmp.join("ptmp");
        // Pre-seed the stale plain dir where the builddir will be.
        let builddir = portage_tmp.join("portage/dev-libs/dirpkg-1.0");
        std::fs::create_dir_all(builddir.join("files")).unwrap();
        let env = compute_environment(&pkg_dir.join("dirpkg-1.0.ebuild"), &portage_tmp).unwrap();
        assert_eq!(
            std::fs::read_link(env.filesdir()).unwrap(),
            pkg_dir.join("files")
        );
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

    /// `FEATURES=compress-build-logs` pump round-trip (real
    /// `EbuildPhase._open_log`'s `gzip.GzipFile(mode="ab")`): bytes
    /// written to the sink's two handles land gzip-encoded in the
    /// `.gz` file -- one member per `open_log_file` call, so a second
    /// call appends a second member -- and decode back losslessly. A
    /// plain path still writes through unencoded.
    #[test]
    fn open_log_file_gzip_pump_round_trips() {
        use std::io::{Read, Write};
        let dir = std::env::temp_dir().join(format!(
            "ebuild-phases-gzlog-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let gz = dir.join("build.log.gz");
        let plain = dir.join("build.log");

        for (path, stdout_text, stderr_text) in [
            (&gz, "phase one out\n", "phase one err\n"),
            (&gz, "phase two out\n", ""),
        ] {
            let mut sink = open_log_file(path).expect("open_log_file succeeds");
            assert!(sink.pump.is_some(), "{path:?} must pump");
            sink.out.write_all(stdout_text.as_bytes()).unwrap();
            sink.err.write_all(stderr_text.as_bytes()).unwrap();
            // Closing the write ends (then joining) finishes the
            // member -- the same drop order the phase call sites use.
            drop(sink);
        }
        let mut sink = open_log_file(&plain).expect("open_log_file succeeds");
        assert!(sink.pump.is_none());
        sink.out.write_all(b"plain\n").unwrap();
        drop(sink);

        let mut decoded = String::new();
        flate2::read::MultiGzDecoder::new(std::fs::File::open(&gz).expect("gz log exists"))
            .read_to_string(&mut decoded)
            .expect("gz log decodes");
        // Both members, both streams -- stdout/stderr chunk order
        // across the two pipes was never deterministic (real appends
        // both fds to one gzip stream too), so compare unordered.
        let mut lines: Vec<&str> = decoded.lines().collect();
        lines.sort_unstable();
        assert_eq!(
            lines,
            vec!["phase one err", "phase one out", "phase two out"],
        );
        assert_eq!(std::fs::read_to_string(&plain).unwrap(), "plain\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Standalone `package.env` build vars (real `_grab_pkg_env`): a
    /// standalone phase env layers a matching `package.env` entry's env
    /// file over the base flags -- atom-matched against the ebuild's own
    /// md5-cache identity, with no resolved graph entry anywhere.
    /// `dev-libs/penvbuildpkg` (mapped to `penv-buildflags`) carries the
    /// env file's `CFLAGS`/`MAKEOPTS` plus real's toolchain selectors
    /// `CC`/`CXX`/`AR`/`RUSTFLAGS` and the `ENV_UNSET` incremental
    /// (backlog #95); `dev-libs/newpkg` (unmapped) doesn't; the `depend`
    /// phase never does (its empty base is deliberate -- metadata
    /// extraction must not see config values).
    #[test]
    fn standalone_phase_env_layers_matching_package_env_build_vars() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let repo = fixtures.join("repo");
        let portage_tmpdir = std::env::temp_dir().join(format!(
            "ebuild-phases-test-{}-standalone-penv",
            std::process::id()
        ));
        let bin_dir = bin_dir().to_path_buf();
        let vars_for = |pkg: &str, pf: &str, phase: &str| {
            let env = compute_environment(
                &repo.join(format!("dev-libs/{pkg}/{pf}.ebuild")),
                &portage_tmpdir,
            )
            .expect("fixture ebuild parses");
            phase_env_vars(
                &env,
                &fixtures,
                phase,
                false,
                &bin_dir,
                &bin_dir.join("ebuild-helpers"),
                &fixtures,
                &[],
            )
        };
        let get = |vars: &[(String, String)], key: &str| {
            vars.iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .next_back()
        };
        let setup_vars = vars_for("penvbuildpkg", "penvbuildpkg-1.0", "setup");
        assert_eq!(
            get(&setup_vars, "CFLAGS").as_deref(),
            Some("-Os -march=fixturepkgenv")
        );
        assert_eq!(get(&setup_vars, "MAKEOPTS").as_deref(), Some("-j7"));
        assert_eq!(get(&setup_vars, "CC").as_deref(), Some("fixture-cc"));
        assert_eq!(get(&setup_vars, "CXX").as_deref(), Some("fixture-cxx"));
        assert_eq!(get(&setup_vars, "AR").as_deref(), Some("fixture-ar"));
        assert_eq!(
            get(&setup_vars, "RUSTFLAGS").as_deref(),
            Some("-C target-cpu=fixturepkg")
        );
        assert_eq!(get(&setup_vars, "ENV_UNSET").as_deref(), Some("PENV_UNSET"));
        let plain_vars = vars_for("newpkg", "newpkg-1.0", "setup");
        assert!(
            !plain_vars
                .iter()
                .any(|(_, v)| v.contains("-march=fixturepkgenv")),
            "unmapped package must not see package.env values"
        );
        let depend_vars = vars_for("penvbuildpkg", "penvbuildpkg-1.0", "depend");
        assert!(
            !depend_vars
                .iter()
                .any(|(_, v)| v.contains("-march=fixturepkgenv")),
            "depend keeps its empty base"
        );
    }

    /// Real `_grab_pkg_env`'s acceptance gate (`config.py:2269-2300`):
    /// every non-`USE` key is accepted unless it is in `env_blacklist`,
    /// `environ_filter`, the profile's dynamic `PROFILE_ONLY_VARIABLES`,
    /// or portuale's own `PORTUALE_COMPUTED` set. The live host capture
    /// behind this matrix is pmtest's `findings/l2.md` "#95 S0".
    #[test]
    fn package_env_accepts_real_scalars_and_rejects_non_user_variables() {
        let profile_only = vec![
            "ARCH".to_string(),
            "ELIBC".to_string(),
            "IUSE_IMPLICIT".to_string(),
            "USE_EXPAND".to_string(),
        ];
        let vars: Vec<(String, String)> = [
            ("CC", "clang"),
            ("CXX", "clang++"),
            ("CPP", "clang-cpp"),
            ("AR", "llvm-ar"),
            ("NM", "llvm-nm"),
            ("RANLIB", "llvm-ranlib"),
            ("LD", "ld.lld"),
            ("RUSTFLAGS", "-C target-cpu=znver3"),
            ("CGO_CFLAGS", "-O2"),
            ("GOFLAGS", "-mod=mod"),
            ("NINJAOPTS", "-j13"),
            ("COMMON_FLAGS", "-O2"),
            ("INSTALL_MASK", "/usr/share/mask"),
            ("PORTAGE_NICENESS", "7"),
            ("__PROBE_LOCAL", "secret"),
            // rejected: env_blacklist
            ("SLOT", "bogus"),
            ("DEPEND", "dev-libs/bogus"),
            ("RDEPEND", "dev-libs/bogus"),
            ("EAPI", "8"),
            ("ROOT", "/"),
            ("PKGUSE", "x"),
            // rejected: global_only_vars + environ_filter
            ("CONFIG_PROTECT", "/protect"),
            // rejected: environ_filter (accepted into real's config,
            // never exported to a phase)
            ("ACCEPT_KEYWORDS", "~amd64"),
            ("CONFIG_PROTECT_MASK", "/mask"),
            ("USE_ORDER", "env"),
            // rejected: `USE` has its own `package_env_use` path
            ("USE", "lto"),
            // rejected: the profile's dynamic PROFILE_ONLY_VARIABLES
            ("ARCH", "x86"),
            ("ELIBC", "musl"),
            ("IUSE_IMPLICIT", "x"),
            ("USE_EXPAND", "VIDEO_CARDS"),
            // rejected: PORTUALE_COMPUTED (FEATURES/PORTAGE_TMPDIR are
            // residues #98/#99; the rest are portuale-owned)
            ("FEATURES", "probe-feature"),
            ("PORTAGE_TMPDIR", "/tmp/probe"),
            ("DISTDIR", "/d"),
            ("PATH", "/bin"),
            ("D", "/d"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let out = match_package_env_vars(
            &[("dev-libs/penvccpkg".to_string(), vars)],
            "dev-libs/penvccpkg-1.0:0/0",
            &profile_only,
            &[],
            &[],
        );
        let keys: Vec<&str> = out.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "CC",
                "CXX",
                "CPP",
                "AR",
                "NM",
                "RANLIB",
                "LD",
                "RUSTFLAGS",
                "CGO_CFLAGS",
                "GOFLAGS",
                "NINJAOPTS",
                "COMMON_FLAGS",
                "INSTALL_MASK",
                "PORTAGE_NICENESS",
                "__PROBE_LOCAL",
            ]
        );
        assert_eq!(
            out.iter().find(|(k, _)| k == "CC").map(|(_, v)| v.as_str()),
            Some("clang")
        );
    }

    /// Real's three-layer incremental semantics: `_grab_pkg_env` appends
    /// within the container (`container[k] += " " + v`), then
    /// `regenerate()` folds `[lower layers, pkg, calling-env]` with
    /// `-*`/`-tok` pruning and a sorted union (`config.py:2735`,
    /// `:2778-2825`) — the calling env folds last, so its `-tok` prunes
    /// a package.env token (S0 cell B) but not vice versa. The three
    /// host captures in pmtest's `findings/l2.md` "#95 S0" are the
    /// expected values for the first three arms (empty calling env).
    #[test]
    fn package_env_incrementals_fold_onto_the_base_like_regenerate() {
        let cpv = "dev-libs/penvccpkg-1.0:0/0";
        let base = vec![(
            "ENV_UNSET".to_string(),
            "BASE_UNSET PROFILE_UNSET".to_string(),
        )];
        let one_file = |v: &str| {
            vec![(
                "dev-libs/penvccpkg".to_string(),
                vec![("ENV_UNSET".to_string(), v.to_string())],
            )]
        };
        // pkg prunes a base token, then the union is sorted.
        let out =
            match_package_env_vars(&one_file("-PROFILE_UNSET PKG_UNSET"), cpv, &[], &base, &[]);
        assert_eq!(
            out,
            vec![("ENV_UNSET".to_string(), "BASE_UNSET PKG_UNSET".to_string())]
        );
        // `-*` clears every base token.
        let out = match_package_env_vars(&one_file("-* ONLY"), cpv, &[], &base, &[]);
        assert_eq!(out, vec![("ENV_UNSET".to_string(), "ONLY".to_string())]);
        // Several matching files of one entry append in order.
        let two_files = vec![(
            "dev-libs/penvccpkg".to_string(),
            vec![
                ("ENV_UNSET".to_string(), "ONE".to_string()),
                ("ENV_UNSET".to_string(), "TWO".to_string()),
            ],
        )];
        let out = match_package_env_vars(&two_files, cpv, &[], &[], &[]);
        assert_eq!(out, vec![("ENV_UNSET".to_string(), "ONE TWO".to_string())]);
        // A `-tok` in the calling-env layer prunes a pkg-layer token
        // (S0 cell B: `FEATURES="-probe-feature"` beats the env file).
        let calling = vec![("ENV_UNSET".to_string(), "-PKG_UNSET".to_string())];
        let out = match_package_env_vars(&one_file("PKG_UNSET"), cpv, &[], &base, &calling);
        assert_eq!(
            out,
            vec![(
                "ENV_UNSET".to_string(),
                "BASE_UNSET PROFILE_UNSET".to_string()
            )]
        );
        // A pkg-layer `-tok` does NOT prune a calling-env token: the
        // calling env folds last and re-adds it (S0 cell B's mirror).
        let calling = vec![("ENV_UNSET".to_string(), "KEEP".to_string())];
        let out = match_package_env_vars(&one_file("-KEEP PKG_UNSET"), cpv, &[], &base, &calling);
        assert_eq!(
            out,
            vec![(
                "ENV_UNSET".to_string(),
                "BASE_UNSET KEEP PKG_UNSET PROFILE_UNSET".to_string()
            )]
        );
    }

    /// Scalars are set, not appended: a later file replaces, and an empty
    /// value blanks the variable (real `CC=""` in an env file leaves the
    /// phase's `CC` empty, S0 capture). A non-matching atom contributes
    /// nothing.
    #[test]
    fn package_env_scalars_last_wins_and_empty_blanks() {
        let cpv = "dev-libs/penvccpkg-1.0:0/0";
        let files = vec![
            (
                "dev-libs/otherpkg".to_string(),
                vec![("CC".to_string(), "not-me".to_string())],
            ),
            (
                "dev-libs/penvccpkg".to_string(),
                vec![
                    ("CC".to_string(), "first".to_string()),
                    ("CC".to_string(), String::new()),
                    ("CXX".to_string(), "clang++".to_string()),
                ],
            ),
        ];
        let out = match_package_env_vars(&files, cpv, &[], &[], &[]);
        assert_eq!(
            out,
            vec![
                ("CC".to_string(), String::new()),
                ("CXX".to_string(), "clang++".to_string()),
            ]
        );
    }

    /// Real's `env`-over-`pkg` scalar precedence (`USE_ORDER`,
    /// `config.py:1031-1035`): a package.env scalar is dropped when the
    /// calling environment carries the same key, so the process value
    /// the caller already layered wins (backlog #101, S0 cell A). Keys
    /// the calling env does not carry still arrive.
    #[test]
    fn package_env_scalar_loses_to_the_calling_environment() {
        let cpv = "dev-libs/penvccpkg-1.0:0/0";
        let files = vec![(
            "dev-libs/penvccpkg".to_string(),
            vec![
                ("CFLAGS".to_string(), "-Os -march=pkg".to_string()),
                ("CC".to_string(), "pkg-cc".to_string()),
            ],
        )];
        let calling = vec![("CFLAGS".to_string(), "-O2 -pipe".to_string())];
        let out = match_package_env_vars(&files, cpv, &[], &[], &calling);
        assert_eq!(out, vec![("CC".to_string(), "pkg-cc".to_string())]);
    }
}
