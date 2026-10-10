//! Single phases, clean, instprep, and phases run from a saved environment.

use crate::ebuild_phases::*;

/// Builds the `ExecutionParameters` for a brush phase shell, redirecting
/// its stdout+stderr to `log_file` when given (set on the shell's
/// persistent open files so brush's own diagnostics are captured too).
/// The scheduler's *captured* parallel builds run through real `bash`
/// instead (`emerge_build::build_one_source_entry`), where the OS-level
/// fd redirect is complete; this brush path is only reached for a
/// captured build explicitly forced onto the brush backend.
pub(crate) fn brush_phase_params(
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
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
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
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
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
pub(crate) fn seed_saved_environment(
    env: &Environment,
    saved_env_bz2: &Path,
) -> Result<(), String> {
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
pub(crate) fn binary_merge_env() -> Vec<(String, String)> {
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
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
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
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
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
    // Real `dblink.treewalk`'s `REPLACING_VERSIONS` (`vartree.py:4768-
    // 4771`), threaded into the hook env for the binary-merge
    // `pkg_preinst`/`pkg_postinst` (`None` for every unmerge hook, which
    // real does not give the var). See `ebuild_merge::replacing_versions`
    // for the value's source.
    replacing_versions: Option<&str>,
) -> Result<i32, String> {
    let runtime = shared_runtime()?;
    runtime.block_on(async {
        let env = compute_environment(ebuild_path, portage_tmpdir)?;
        create_directories(&env)?;
        if seed {
            seed_saved_environment(&env, saved_env_bz2)?;
        }

        let mut extra_env = binary_merge_env();
        if let Some(version) = replacing_versions {
            extra_env.push(("REPLACING_VERSIONS".to_string(), version.to_string()));
        }
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
