//! The phase and merge shell scripts sent to the client.

use crate::remote::*;

// --- Client merge (slice 4) ----------------------------------------------------

/// Real `eapi_exports_merge_type` (`lib/portage/eapi.py:63`, attrs table
/// `:300`: `exports_merge_type = eapi >= Eapi("4")`): `MERGE_TYPE` reaches
/// the phase env only for EAPI 4+ (real `config.py:3335-3336` pops it
/// otherwise). An unsupported or unparseable EAPI takes real's
/// `_get_eapi_attrs` fallback row (`eapi.py:247-258`), which exports.
pub(crate) fn eapi_exports_merge_type(eapi: &str) -> bool {
    // Real tests the *whole* string against `_supported_eapis`; anything
    // that is not a plain supported number (e.g. `3-foo`) takes the
    // exporting fallback row.
    match eapi.trim().parse::<u32>() {
        Ok(n) => n >= 4,
        Err(_) => true,
    }
}

/// `export` block for the new instance's phase script (`phase_script`;
/// the replaced instance's prerm/postrm go through the merge driver's own
/// `run_old_hook`, which deliberately exports no `MERGE_TYPE`: real's
/// unmerge re-creates the old instance's settings, `vartree.py:4535-4540`,
/// and never passes through the scheduler that sets it): path overrides with
/// caller-side values, everything else from the sourced saved env.
/// `unit_bin` is the shipped runtime dir (`$UNIT/bin`,
/// `$OLD_TMP/bin` symlink or copy -- callers decide).
/// `MERGE_TYPE=binary` rides the same line as `EMERGE_FROM` (real
/// `_emerge/Binpkg.py:92` sets it for every binpkg merge; `mrg` only
/// merges binpkgs), gated on [`eapi_exports_merge_type`].
/// `portuale_bin` is the S8.1-resolved absolute client path (#326 D5):
/// every client call uses it, never `$PATH` -- `PORTAGE_PYTHON` is the
/// shipped `portuale-python` shim through the script's own `$UNITBIN`
/// (`$UNIT/bin`, defined by each script header), `PORTAGE_PYM_PATH` is
/// `/` (no checkout on the client), and `PORTAGE_IPC_DAEMON` is unset
/// (no IPC daemon, #326 D6 -- this also scrubs a value inherited over
/// the local transport).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn phase_exports(
    ebuild: &str,
    build_dir: &str,
    root: &str,
    image_dir: &str,
    temp_dir: &str,
    work_subdir: &str,
    home_dir: &str,
    files_dir: &str,
    bin_dir: &str,
    tmpdir: &str,
    colormap: &str,
    staged: &crate::remote_bundle::StagedBundle,
    phase: &str,
    portuale_bin: &str,
) -> String {
    format!(
        concat!(
            "export EAPI={eapi} CATEGORY={category} PN={pn} PV={pv} PR={pr} PVR={pvr} P={p} PF={pf}\n",
            "export EBUILD={ebuild} O={obuild}\n",
            "export ROOT={root} EROOT={root}\n",
            "export PORTAGE_BUILDDIR={builddir}\n",
            "export WORKDIR={workdir}\n",
            "export D={imaged}/ ED={imaged}/\n",
            "export T={temp} HOME={home} FILESDIR={filesdir}\n",
            "export PORTAGE_BIN_PATH={bindir}\n",
            "export PORTAGE_ECLASS_LOCATIONS=\"\"\n",
            "export PORTUALE_BIN={portuale_bin}\n",
            "export PORTAGE_PYM_PATH=/\n",
            "unset PORTAGE_IPC_DAEMON\n",
            "export PORTAGE_PYTHON=\"$UNITBIN/portuale-python\"\n",
            "export PORTAGE_COLORMAP={colormap}\n",
            "export PORTAGE_TMPDIR={tmpdir}\n",
            "export SANDBOX_LOG={temp}/sandbox.log\n",
            "export EBUILD_PHASE={phase} EMERGE_FROM=binary{merge_type}\n",
        ),
        merge_type = if eapi_exports_merge_type(&staged.eapi) {
            " MERGE_TYPE=binary"
        } else {
            ""
        },
        eapi = sh_quote(&staged.eapi),
        category = sh_quote(&staged.category),
        pn = sh_quote(&staged.pn),
        pv = sh_quote(&staged.pv),
        pr = sh_quote(&staged.pr),
        pvr = sh_quote(&staged.pvr),
        p = sh_quote(&staged.p),
        pf = sh_quote(&staged.pf),
        ebuild = sh_quote(ebuild),
        obuild = sh_quote(&format!("{build_dir}/build-info")),
        root = sh_quote(root),
        builddir = sh_quote(build_dir),
        workdir = sh_quote(work_subdir),
        imaged = sh_quote(image_dir),
        temp = sh_quote(temp_dir),
        home = sh_quote(home_dir),
        filesdir = sh_quote(files_dir),
        bindir = sh_quote(bin_dir),
        tmpdir = sh_quote(tmpdir),
        colormap = sh_quote(colormap),
        phase = phase,
        portuale_bin = sh_quote(portuale_bin),
    )
}

// --- Client phases (slice 3) -------------------------------------------------

/// One phase invocation, generated bash: recreate the local
/// `run_phase_from_saved_env` + `run_one_phase_bash` setup client-side
/// (fresh `bash bin/ebuild.sh <phase>` process per phase -- the readonly
/// `EBUILD_PHASE` semantics demand it, exactly like local `spawnebuild`).
/// Path overrides are exported with client-side values; everything else
/// (EAPI, PN/PV/…, USE) rides the sourced saved environment, whose stale
/// path copies ebuild.sh's own `__preprocess_ebuild_env` strips (the
/// `environment.raw` marker enables that filtering, mirroring local).
/// `S` is deliberately *not* exported: ebuild.sh defaults it to
/// `${WORKDIR}/${P}` with `P` from the saved env (exact local behavior).
/// `PHASE_<phase>=<rc>` on stdout, phase log raw on stdout/stderr.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn phase_script(
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    phase: &str,
    root: &str,
    workdir_parent: &str,
    colormap: &str,
    portuale_bin: &str,
) -> String {
    // D keeps local's trailing slash. `UNITBIN` is the shipped runtime
    // dir (`$UNIT/bin`): `phase_exports` resolves `PORTAGE_PYTHON`
    // through it, so no shim falls back to `PATH` (#326 D5/S8.3).
    format!(
        concat!(
            "UNIT={unit}\n",
            "UNITBIN=\"$UNIT/bin\"\n",
            "T=\"$UNIT/temp\"\n",
            "mkdir -p \"$T\" \"$UNIT/work\" \"$UNIT/homedir\" \"$UNIT/files\" \"$UNIT/empty\"\n",
            "cp \"$UNIT/environment\" \"$T/environment\"\n",
            ": > \"$T/environment.raw\"\n",
            "{exports}",
            "export PATH=\"$UNIT/bin/ebuild-helpers:$PATH\"\n",
            "bash \"$UNIT/bin/ebuild.sh\" {phase}\n",
            "rc=$?\n",
            "echo \"PHASE_{phase}=$rc\"\n",
            "exit $rc\n",
        ),
        unit = sh_quote(unit_dir),
        exports = phase_exports(
            &format!("{unit_dir}/build-info/{}.ebuild", staged.pf),
            unit_dir,
            root,
            &format!("{unit_dir}/image"),
            &format!("{unit_dir}/temp"),
            &format!("{unit_dir}/work"),
            &format!("{unit_dir}/homedir"),
            &format!("{unit_dir}/files"),
            &format!("{unit_dir}/bin"),
            workdir_parent,
            colormap,
            staged,
            phase,
            portuale_bin,
        ),
        phase = phase,
    )
}

/// Locale variables the regen postinst run re-exports into the client
/// phase env: real's saved locale comes from the calling environment
/// through `environ_whitelist` (`special_env_vars.py`: `LANG` plus the
/// `LC_*` list), and for `mrg` the calling environment is the server
/// process. `LANGUAGE` is **not** in real's whitelist -- it rides along
/// here only because the slice brief names it explicitly; see
/// `collect_server_locale`. Only set variables are ever forwarded
/// (never invented defaults).
pub(crate) const REGEN_LOCALE_VARS: &[&str] = &[
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_PAPER",
];

/// The server process's own locale values for the client regen phase
/// env, as `(name, value)` pairs in `REGEN_LOCALE_VARS` order -- one
/// entry per variable the getter reports as set (even empty: a set-but-
/// empty `LANG` is still the calling environment's value, which is what
/// real's whitelist passes through). Production passes
/// `|name| std::env::var(name).ok()`; tests inject a fake map.
pub(crate) fn collect_server_locale(get: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    REGEN_LOCALE_VARS
        .iter()
        .filter_map(|name| get(name).map(|value| (name.to_string(), value)))
        .collect()
}

/// Real's `posixish_locale` EAPI attribute (`eapi.py:182,309`): true
/// from EAPI 6 on. Only posixish phases run the `LC_ALL` split below
/// (`EbuildPhase.py:51-56`); older EAPIs keep `LC_ALL` as-is.
pub(crate) fn eapi_is_posixish(eapi: &str) -> bool {
    let digits: String = eapi
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    // Real rejects an unknown EAPI outright; anything reaching us parsed
    // off an ebuild, so default an unparseable value to the modern rule
    // (the same direction the local phase path's unconditional split
    // already takes).
    digits.parse::<u64>().map(|n| n >= 6).unwrap_or(true)
}

/// Real's `locale_categories` (`portage/util/locale.py:22-36`): the full
/// set `split_LC_ALL` copies a set `LC_ALL` over -- the six POSIX
/// categories, `LC_PAPER`, and the five GNU extensions
/// (`LC_ADDRESS`, `LC_IDENTIFICATION`, `LC_MEASUREMENT`, `LC_NAME`,
/// `LC_TELEPHONE`). The fan-out below iterates this list, not the
/// forwarding set: real's split operates on settings, and for a
/// binpkg-merge postinst `$T/environment` does not exist yet, so
/// `config.environ()` applies no whitelist filter
/// (`config.py:3286-3299`) and all twelve reach the phase env and the
/// `PORTAGE_UPDATE_ENV` save (`EbuildPhase.py:158,259` runs the env
/// extractor for `pretend`/`prerm` only). Probed against real's own
/// `split_LC_ALL` + `environ()`: with `LC_ALL` set, all twelve export
/// when `$T/environment` is absent, seven when present.
pub(crate) const PORTAGE_LOCALE_CATEGORIES: &[&str] = &[
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
];

/// Port of real's `split_LC_ALL` (`portage/util/locale.py:160`) to the
/// forwarded regen locale set, for a posixish phase: a set `LC_ALL` fans
/// out over all twelve real `locale_categories` (see
/// `PORTAGE_LOCALE_CATEGORIES`; real copies it over the same list,
/// unconditionally overwriting) and itself
/// disappears (real blanks it, then `config.environ()` deletes the
/// placeholder, `config.py:3374-3385`). `LANG` is filled from `LC_ALL`
/// only when the server left it unset -- real's split never touches
/// `LANG`, but with `LC_ALL` set and `LANG` unset the effective locale
/// is `LC_ALL`'s value everywhere, and the bed shows real's saved env
/// carrying `LANG` alongside the split categories. An explicitly set
/// `LANG` is never clobbered; `LANGUAGE` (not a locale category, rides
/// along per the l171b brief) is never touched. A set-but-empty
/// `LC_ALL` is dropped without fanning out (real's `if lc_all:` is
/// falsy, then `environ()` deletes the placeholder). Non-posixish
/// EAPIs return the pairs unchanged (real never calls the split there).
pub(crate) fn split_server_locale(
    locale: &[(String, String)],
    eapi: &str,
) -> Vec<(String, String)> {
    if !eapi_is_posixish(eapi) {
        return locale.to_vec();
    }
    let lc_all = locale
        .iter()
        .rev()
        .find(|(name, _)| name == "LC_ALL")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    let mut out: Vec<(String, String)> = locale
        .iter()
        .filter(|(name, _)| name != "LC_ALL")
        .cloned()
        .collect();
    if lc_all.is_empty() {
        return out;
    }
    for name in PORTAGE_LOCALE_CATEGORIES {
        match out.iter_mut().find(|(n, _)| n == name) {
            Some(pair) => pair.1 = lc_all.clone(),
            None => out.push((name.to_string(), lc_all.clone())),
        }
    }
    if !out.iter().any(|(n, _)| n == "LANG") {
        out.push(("LANG".to_string(), lc_all));
    }
    out
}

/// The merge-time environment-regeneration postinst run (backlog #171):
/// real `vartree.py:5334` sets `PORTAGE_UPDATE_ENV=<dbpkgdir>/
/// environment.bz2` around *every* merge's postinst phase, and
/// `bin/phase-functions.sh:1072-1082` re-saves the vdb env from the
/// live phase env through it -- so the vdb env carries the merge-time
/// config, not the binpkg's build-time one. `mrg` runs this phase on
/// the client for every merged unit, **whether or not `pkg_postinst`
/// is defined**: real's `prerm|postrm|preinst|postinst|config|info`
/// case (`phase-functions.sh:1061-1083`) runs
/// `__ebuild_phase_with_hooks pkg_${1}` unconditionally
/// (`__ebuild_phase` is a no-op for an undefined function) and *then*
/// the `PORTAGE_UPDATE_ENV` block -- so an undefined `pkg_postinst`
/// still regenerates the env. `pkg_postinst` itself still runs only if
/// defined (the same no-op rule; nothing faked).
///
/// `PORTAGE_UPDATE_ENV` points at a unit-local file
/// (`$UNIT/environment.regen`) and `PORTAGE_BZIP2_COMMAND` at the
/// bundle's `bzip2-passthrough` (plain text out; the server compresses
/// -- the client must have no `bzip2`, plan §6). `FEATURES` /
/// `PORTAGE_FEATURES` carry the resolved client list (the local
/// `refresh_features` rule), so the env records the merge-time
/// features (e.g. no `buildpkg`). `locale` re-exports the server's own
/// locale values (see `REGEN_LOCALE_VARS` / `collect_server_locale`,
/// split per `split_server_locale` on posixish EAPIs) so
/// the regen'd env carries them exactly as real's calling-environment
/// whitelist does. `PHASE_postinst=<rc>` plus
/// `REGEN_ENV=ok|missing|skip:no-hooks` come back on stdout; the
/// script exits with the phase rc (a non-zero postinst is non-fatal --
/// real `_postinst_failure` -- as long as the regen file exists).
///
/// `O` is unset and `SHELL` unset before the phase runs: real's
/// phase shell never has `O` (`config.environ()` drops it via
/// `special_env_vars.py: environ_filter` even though `doebuild.py:475`
/// sets `mysettings["O"]`) and its saved `SHELL` is `declare --`
/// (bash initializes its own -- `SHELL` is in neither the whitelist
/// nor the config), while `mrg` exported a unit-local `O` and usually
/// inherits an exported `SHELL`. `LC_ALL` is unset on posixish EAPIs
/// and the forwarded locale arrives pre-split (see
/// `split_server_locale`); the unset also kills any `LC_ALL` leaking
/// in from the sourced build-time environment.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn postinst_regen_script(
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    root: &str,
    workdir_parent: &str,
    colormap: &str,
    features: Option<&str>,
    locale: &[(String, String)],
    portuale_bin: &str,
) -> String {
    let mut extra = String::new();
    if let Some(list) = features.filter(|f| !f.is_empty()) {
        extra.push_str(&format!(
            "export FEATURES={features} PORTAGE_FEATURES={features}\n",
            features = sh_quote(list),
        ));
    }
    extra.push_str(&format!(
        "export PORTAGE_UPDATE_ENV={regen} PORTAGE_BZIP2_COMMAND={passthrough}\n",
        regen = sh_quote(&format!("{unit_dir}/environment.regen")),
        passthrough = sh_quote(&format!("{unit_dir}/bin/bzip2-passthrough")),
    ));
    // Backlog #171b: match real's phase shell (see the fn doc comment)
    // so the `PORTAGE_UPDATE_ENV` save matches real's saved env.
    extra.push_str("unset O\n");
    // Backlog #171c: `unset` (not `export -n`) reproduces real exactly:
    // bash self-initializes an unexported `SHELL`, while `export -n`
    // would keep a non-default server value.
    extra.push_str("unset SHELL\n");
    // Backlog #171c: real's posixish phases never see `LC_ALL`
    // (`split_LC_ALL` + `config.environ()`); the unset also covers an
    // `LC_ALL` leaking in from the sourced build-time environment, and
    // the exports below carry the pre-split values.
    let posixish = eapi_is_posixish(&staged.eapi);
    if posixish {
        extra.push_str("unset LC_ALL\n");
    }
    for (name, value) in split_server_locale(locale, &staged.eapi).iter() {
        extra.push_str(&format!(
            "export {name}={quoted}\n",
            quoted = sh_quote(value)
        ));
    }
    format!(
        concat!(
            "UNIT={unit}\n",
            "UNITBIN=\"$UNIT/bin\"\n",
            "T=\"$UNIT/temp\"\n",
            "mkdir -p \"$T\" \"$UNIT/work\" \"$UNIT/homedir\" \"$UNIT/files\" \"$UNIT/empty\"\n",
            "if [ ! -f \"$UNIT/build-info/{pf}.ebuild\" ] || [ ! -f \"$UNIT/environment\" ]; then\n",
            "  echo \"REGEN_ENV=skip:no-hooks\"\n",
            "  echo \"PHASE_postinst=0\"\n",
            "  exit 0\n",
            "fi\n",
            "cp \"$UNIT/environment\" \"$T/environment\"\n",
            ": > \"$T/environment.raw\"\n",
            "{exports}",
            "{extra}",
            "export PATH=\"$UNIT/bin/ebuild-helpers:$PATH\"\n",
            "bash \"$UNIT/bin/ebuild.sh\" postinst\n",
            "rc=$?\n",
            "echo \"PHASE_postinst=$rc\"\n",
            "if [ -s \"$UNIT/environment.regen\" ]; then echo \"REGEN_ENV=ok\"; else echo \"REGEN_ENV=missing\"; fi\n",
            "exit $rc\n",
        ),
        unit = sh_quote(unit_dir),
        pf = staged.pf,
        exports = phase_exports(
            &format!("{unit_dir}/build-info/{}.ebuild", staged.pf),
            unit_dir,
            root,
            &format!("{unit_dir}/image"),
            &format!("{unit_dir}/temp"),
            &format!("{unit_dir}/work"),
            &format!("{unit_dir}/homedir"),
            &format!("{unit_dir}/files"),
            &format!("{unit_dir}/bin"),
            workdir_parent,
            colormap,
            staged,
            "postinst",
            portuale_bin,
        ),
        extra = extra,
    )
}

// --- Client merge driver (slice 4): helpers ----------------------------------

/// Longest-prefix `is_protected` + `alloc_cfg` + `env_val` + `run_old_hook`
/// helpers shared by the merge flow. Pure bash, no placeholders except
/// the colormap/PATH roots baked by the caller template below.
pub(crate) const MERGE_HELPERS: &str = include_str!("merge_helpers.sh");

// --- Client merge flow (slice 4) -----------------------------------------------

/// Merge flow: old-prerm → ownership scan → gate+copy → vdb → remove-old
/// → old-postrm → env-update, appended after `MERGE_HELPERS`. Shell vars
/// (`UNIT`, `ROOT`, `VDB`, `VDBROOT`, `NEWPF`, `PKG`, `MAINS`, `PROTECT`,
/// `MASK`, `STATELESS`, …) come from `merge_script`'s header. Machine
/// lines `MERGE_<STEP>=…` plus a final `STATUS=merged` (or
/// `STATUS=failed:<step>` from `mfail`); any `mfail` prints
/// `MERGE_FAIL=<step> <detail>` and exits 1. `STATELESS=1` (server-side
/// vdb placement) skips old-version discovery and the ownership scan --
/// any existing file/symlink destination (outside CONFIG_PROTECT
/// divert) is a fail-closed collision, and old hooks never run.
/// New-postinst runs separately afterwards (see `run_bundle_stage`).
///
/// The `client:<path>` VDB is read and written by this bash on the
/// client, so it stays in the files format by construction: portuale
/// has no code there to call `portage-vdb` from (feat#157 design §6.3).
/// Only the server-side `VdbShadow` read goes through `portage-vdb`.
pub(crate) const MERGE_FLOW: &str = include_str!("merge_flow.sh");

/// Merge header: all values the flow needs, pre-quoted. The unit layout
/// mirrors the local merge (`image/`, `build-info/`, shipped `bin/`).
/// Client ledger destination + line baked into the merge driver.
/// `None` file skips the client record (the `--remote-binpkg` trial path
/// keeps no ledger; the resolve path always records).
/// `portuale_bin` is the S8.1-resolved absolute client path (#326 D5),
/// exported for the whole driver (including the old hooks, which inherit
/// the header's exports); `PORTAGE_PYM_PATH` is `/` and
/// `PORTAGE_IPC_DAEMON` is unset, like the phase scripts.
#[derive(Debug, Clone)]
pub(crate) struct LedgerSpec {
    pub file: String,
    pub line: String,
}

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn merge_script(
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    root: &str,
    vdb: &str,
    stateless: bool,
    protect_list: &str,
    mask_list: &str,
    ledger: Option<&LedgerSpec>,
    portuale_bin: &str,
) -> Result<String, String> {
    // A placed client vdb must stay inside the unit's world: reject the
    // degenerate empty path before it becomes `//var/db/pkg`.
    if vdb.trim().is_empty() {
        return Err("mrg: empty client vdb path".to_string());
    }
    Ok(format!(
        concat!(
            "UNIT={unit}\n",
            "IMAGE=\"$UNIT/image\"\n",
            "VDB={vdb}\n",
            "VDBROOT={vdb}/{category}\n",
            "STATELESS={stateless}\n",
            "CAT={category}\n",
            "PKG={pkg}\n",
            "NEWPF={pf}\n",
            "NEWVDB=\"$VDBROOT/$NEWPF\"\n",
            "TMPVDB=\"$VDBROOT/-MERGING-$NEWPF\"\n",
            "FULLSLOT={slot}\n",
            "MAINS={mainslot}\n",
            "REPO={repo}\n",
            "ROOT={root}\n",
            "WORKDIR={workdir}\n",
            "UNITBIN=\"$UNIT/bin\"\n",
            "export PORTUALE_BIN={portuale_bin}\n",
            "export PORTAGE_PYM_PATH=/\n",
            "unset PORTAGE_IPC_DAEMON\n",
            "COLORMAP={colormap}\n",
            "PROTECT={protect}\n",
            "MASK={mask}\n",
            "LEDGER_FILE={ledger_file}\n",
            "LEDGER_LINE={ledger_line}\n",
            "NEWCONTENTS=\"$UNIT/CONTENTS.new\"\n",
            "NEWPATHS=\"$UNIT/paths.new\"\n",
            "REPLACED_OWN=\"$UNIT/replaced.own\"\n",
            "OTHERS_OWN=\"$UNIT/others.own\"\n",
            "{helpers}",
            "{flow}",
        ),
        unit = sh_quote(unit_dir),
        root = sh_quote(root),
        vdb = sh_quote(vdb),
        stateless = if stateless { "1" } else { "0" },
        category = sh_quote(&staged.category),
        pkg = sh_quote(&staged.pn),
        pf = sh_quote(&staged.pf),
        slot = sh_quote(&staged.manifest.slot),
        mainslot = sh_quote(staged.manifest.slot.split('/').next().unwrap_or("0")),
        repo = sh_quote(&staged.manifest.repo),
        workdir = sh_quote(
            &std::path::Path::new(unit_dir)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "/var/tmp".to_string())
        ),
        colormap = sh_quote(&crate::color::phase_colormap_export()),
        protect = sh_quote(protect_list),
        mask = sh_quote(mask_list),
        ledger_file = sh_quote(&ledger.map(|l| l.file.clone()).unwrap_or_default()),
        ledger_line = sh_quote(&ledger.map(|l| l.line.clone()).unwrap_or_default()),
        helpers = MERGE_HELPERS,
        flow = MERGE_FLOW,
        portuale_bin = sh_quote(portuale_bin),
    ))
}
