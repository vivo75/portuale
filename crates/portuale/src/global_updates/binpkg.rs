//! The binpkg half of `_do_global_updates`: `move_ent` / `move_slot_ent` over
//! `$PKGDIR` files (xpak and gpkg), `update_ents` over their embedded
//! `*DEPEND` metadata, and the `Packages` index rewrite.
// Portage: lib/portage/_global_updates.py, _do_global_updates

use crate::binpkg::{
    MAX_INNER_METADATA_BYTES, OuterMember, bytes_hex_digest, classify_inner_member, file_mtime,
    outer_member_read, parse_xpak_members, read_gpkg_metadata, read_inner_metadata,
    read_member_bytes_capped, read_outer_members, read_xpak_segment, verify_gpkg_manifest,
};
use crate::ebuild_package::{
    CompressionArgvArgs, compression_command_argv, format_packages_entry, makeopts_to_job_count,
    omit_stanza_default, sort_stanza_fields_for_write, write_packages_gz,
};
use crate::global_updates::vdb::DEP_KEYS;
use crate::global_updates::{RepoMatch, collapse_ws};
use crate::helpers::digest_of;
use crate::helpers::xpak::{recompose_mem, xpak_mem};
use crate::portage_lock::PortageLockfile;
use portage_repo::{RepoConfig, UpdateCmd, parse_packages_index};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What [`apply_binpkg_moves`] needs: the `$PKGDIR` whose files move, the
/// repos (in `portdb` order, for the main-repo match) with the gated updates
/// map the driver scanned, the settings halves the writers need
/// (`binpkg-multi-instance` for the allocated path, `BINPKG_COMPRESS`
/// (+ flags, `MAKEOPTS`, bzip2 command) for the gpkg metadata member,
/// `compress-index` for `Packages.gz`), and the progress hook the output
/// driver passes (`None` in tests that do not watch progress): `'%'` per
/// `move` count, `'S'` per `slotmove` count.
#[cfg(test)]
pub(crate) struct BinpkgMovesArgs<'a> {
    pub pkgdir: &'a Path,
    pub repos: &'a [RepoConfig],
    pub map: &'a [(String, Vec<UpdateCmd>)],
    pub multi_instance: bool,
    pub binpkg_compress: Option<&'a str>,
    pub binpkg_compress_flags: Option<&'a str>,
    pub portage_bzip2_command: Option<&'a str>,
    pub makeopts: Option<&'a str>,
    pub compress_index: bool,
    pub on_progress: Option<&'a dyn Fn(char, usize)>,
}

/// How many binpkgs [`apply_binpkg_moves`] copied (`move`) and rewrote
/// in place (`slotmove`).
#[cfg(test)]
pub(crate) struct BinpkgMoveReport {
    pub moves: usize,
    pub slot_moves: usize,
}

/// What [`update_binpkg_ents`] needs: the `$PKGDIR` whose embedded dep
/// strings move, the repos with the gated updates map, the same writer
/// settings as the moves, and the progress hook the output driver passes
/// (`None` in tests that do not watch progress).
pub(crate) struct UpdateBinpkgEntsArgs<'a> {
    pub pkgdir: &'a Path,
    pub repos: &'a [RepoConfig],
    pub map: &'a [(String, Vec<UpdateCmd>)],
    pub binpkg_compress: Option<&'a str>,
    pub binpkg_compress_flags: Option<&'a str>,
    pub portage_bzip2_command: Option<&'a str>,
    pub makeopts: Option<&'a str>,
    pub compress_index: bool,
    pub on_update: Option<&'a dyn Fn(usize, usize)>,
}

/// Apply every repo's `move`/`slotmove` to the `$PKGDIR` files, holding no
/// vdb lock (binpkgs live outside the vdb). A non-writable `PKGDIR`
/// skips everything, like Portage's `bindb.writable` gate; a missing or
/// unreadable file, a non-matching `repository`, an already-applied move
/// and an occupied destination skip the entry; a signed gpkg is skipped
/// on a move and removed on an `aux_update`. The `Packages` index is
/// rewritten after every file change.
///
/// # Errors
///
/// Returns `Err` when a file cannot be read, rewritten, moved or
/// indexed.
// Portage: lib/portage/_global_updates.py, _do_global_updates
#[cfg(test)]
pub(crate) fn apply_binpkg_moves(args: BinpkgMovesArgs<'_>) -> Result<BinpkgMoveReport, String> {
    if !pkgdir_writable(args.pkgdir) {
        return Ok(BinpkgMoveReport {
            moves: 0,
            slot_moves: 0,
        });
    }
    let map = args.map;
    let master = args
        .repos
        .iter()
        .find(|r| r.is_main)
        .map(|r| r.name.clone());
    let matcher = RepoMatch::new(map, master);
    let mut state = BinpkgState::load(args.pkgdir)?;
    let mut report = BinpkgMoveReport {
        moves: 0,
        slot_moves: 0,
    };
    let writer = WriterConfig::new(WriterConfigArgs {
        multi_instance: args.multi_instance,
        binpkg_compress: args.binpkg_compress,
        binpkg_compress_flags: args.binpkg_compress_flags,
        portage_bzip2_command: args.portage_bzip2_command,
        makeopts: args.makeopts,
        compress_index: args.compress_index,
        now: now_secs(),
    });
    for (repo_name, cmds) in map {
        if repo_name == "DEFAULT" || cmds.is_empty() {
            continue;
        }
        for cmd in cmds {
            let (moves, slot_moves) = apply_binpkg_cmd(BinpkgCmdArgs {
                state: &mut state,
                writer: &writer,
                repo_name,
                matcher: &matcher,
                cmd,
                on_progress: args.on_progress,
            })?;
            report.moves += moves;
            report.slot_moves += slot_moves;
        }
    }
    Ok(report)
}

/// What [`apply_binpkg_cmd`] needs: the loaded index state, the writer
/// halves, the update repo's name with the shared matcher, the one
/// command to apply, and the progress hook (`'%'`/`'S'` with the count,
/// only when nonzero).
pub(crate) struct BinpkgCmdArgs<'a> {
    pub state: &'a mut BinpkgState,
    pub writer: &'a WriterConfig<'a>,
    pub repo_name: &'a str,
    pub matcher: &'a RepoMatch,
    pub cmd: &'a UpdateCmd,
    pub on_progress: Option<&'a dyn Fn(char, usize)>,
}

/// One `move`/`slotmove` over the `$PKGDIR` files, reporting how many
/// binpkgs it copied (`move`) and rewrote in place (`slotmove`). The
/// progress hook fires at the same site Portage writes the character,
/// so the driver prints `'%'`/`'S'` interleaved with the vdb half's
/// `'@'`/`'s'`.
///
/// # Errors
///
/// Returns `Err` when a file cannot be read, rewritten, moved or
/// indexed.
// Portage: lib/portage/_global_updates.py, _do_global_updates
pub(crate) fn apply_binpkg_cmd(args: BinpkgCmdArgs<'_>) -> Result<(usize, usize), String> {
    let repo_match = |repository: &str| args.matcher.matches(args.repo_name, repository);
    match args.cmd {
        UpdateCmd::Move { old, new } => {
            let moves = move_binpkg(args.state, old, new, &repo_match, args.writer)?;
            if moves > 0
                && let Some(on_progress) = args.on_progress
            {
                on_progress('%', moves);
            }
            Ok((moves, 0))
        }
        UpdateCmd::SlotMove {
            cp,
            old_slot,
            new_slot,
            atom,
        } => {
            let slot_moves = move_binpkg_slot(
                args.state,
                MoveSlotArgs {
                    cp,
                    old_slot,
                    new_slot,
                    atom: atom.as_deref(),
                },
                &repo_match,
                args.writer,
            )?;
            if slot_moves > 0
                && let Some(on_progress) = args.on_progress
            {
                on_progress('S', slot_moves);
            }
            Ok((0, slot_moves))
        }
    }
}

/// `dbapi.update_ents` over the binpkgs: every file's `*DEPEND` metadata
/// is read through the `aux_get` whitespace collapse, rewritten with the
/// entry's own repo updates, and written back in place when changed. The
/// `Packages` index is rewritten after every file change. Returns how
/// many files changed.
///
/// # Errors
///
/// Returns `Err` when a file cannot be read, rewritten or indexed.
// Portage: lib/portage/dbapi/__init__.py, update_ents
pub(crate) fn update_binpkg_ents(args: UpdateBinpkgEntsArgs<'_>) -> Result<usize, String> {
    if !pkgdir_writable(args.pkgdir) {
        return Ok(0);
    }
    let map = args.map;
    let master = args
        .repos
        .iter()
        .find(|r| r.is_main)
        .map(|r| r.name.clone());
    let matcher = RepoMatch::new(map, master);
    let mut state = BinpkgState::load(args.pkgdir)?;
    let mut cpvs = state.sorted_cpvs();
    cpvs.sort_by(|a, b| cmp_cpv(a, b));
    let maxval = cpvs.len();
    if let Some(on_update) = args.on_update {
        on_update(maxval, 0);
    }
    let writer = WriterConfig::new(WriterConfigArgs {
        multi_instance: false,
        binpkg_compress: args.binpkg_compress,
        binpkg_compress_flags: args.binpkg_compress_flags,
        portage_bzip2_command: args.portage_bzip2_command,
        makeopts: args.makeopts,
        compress_index: args.compress_index,
        now: now_secs(),
    });
    let mut updated = 0usize;
    for (pos, cpv) in cpvs.iter().enumerate() {
        let entry = state.entry(cpv);
        let Some(entry) = entry else {
            continue;
        };
        let meta = match read_file_meta(args.pkgdir, &entry) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        let repository = meta.repository().unwrap_or_else(|| entry.repo.clone());
        let cmds: &[UpdateCmd] = map
            .iter()
            .find_map(|(name, cmds)| {
                if name != "DEFAULT" && matcher.matches(name, &repository) {
                    Some(cmds.as_slice())
                } else {
                    None
                }
            })
            .unwrap_or(&[]);
        if cmds.is_empty() {
            continue;
        }
        let mut rewritten: Vec<(String, String)> = Vec::new();
        for key in DEP_KEYS {
            let collapsed = collapse_ws(meta.value(key).unwrap_or(""));
            if let Some(new) = portage_repo::apply_cmds_to_dep_string(&collapsed, cmds) {
                rewritten.push(((*key).to_string(), new));
            }
        }
        if rewritten.is_empty() {
            continue;
        }
        aux_update(&mut state, &entry, &rewritten, &writer)?;
        updated += 1;
        if let Some(on_update) = args.on_update {
            on_update(maxval, pos + 1);
        }
    }
    Ok(updated)
}

/// The writer halves of the settings the driver resolves: the
/// multi-instance layout switch, the gpkg metadata compressor (name,
/// flags, bzip2 command, `MAKEOPTS` for `{JOBS}`) and the `Packages.gz`
/// switch, plus one clock reading per top-level call.
pub(crate) struct WriterConfig<'a> {
    multi_instance: bool,
    binpkg_compress: Option<&'a str>,
    binpkg_compress_flags: Option<&'a str>,
    portage_bzip2_command: Option<&'a str>,
    makeopts: Option<&'a str>,
    compress_index: bool,
    now: u64,
}

/// What `WriterConfig::new` needs: the writer halves above.
pub(crate) struct WriterConfigArgs<'a> {
    pub multi_instance: bool,
    pub binpkg_compress: Option<&'a str>,
    pub binpkg_compress_flags: Option<&'a str>,
    pub portage_bzip2_command: Option<&'a str>,
    pub makeopts: Option<&'a str>,
    pub compress_index: bool,
    pub now: u64,
}

impl<'a> WriterConfig<'a> {
    /// Bundle the writer halves with one clock reading.
    pub(crate) fn new(args: WriterConfigArgs<'a>) -> Self {
        Self {
            multi_instance: args.multi_instance,
            binpkg_compress: args.binpkg_compress,
            binpkg_compress_flags: args.binpkg_compress_flags,
            portage_bzip2_command: args.portage_bzip2_command,
            makeopts: args.makeopts,
            compress_index: args.compress_index,
            now: args.now,
        }
    }
}

/// What [`move_binpkg_slot`] needs beyond the shared state, matcher and
/// writer: the package, the slot rewrite, and the versioned atom text
/// (`None` for the bare form, which matches every version).
struct MoveSlotArgs<'a> {
    cp: &'a (String, String),
    old_slot: &'a str,
    new_slot: &'a str,
    atom: Option<&'a str>,
}

/// What the moved file's write-out needs beyond the writer: the source,
/// the `.partial` rewrite target, the allocated destination, the rewritten
/// pairs, the source format and the new container prefix.
struct WriteMovedArgs<'a> {
    full_path: &'a Path,
    update_path: &'a Path,
    allocated: &'a Path,
    pairs: &'a [(Vec<u8>, Vec<u8>)],
    meta: &'a FileMeta,
    basename: String,
}

/// What the path allocator needs beyond the index state: the new package,
/// its file name, the format suffix, the inherited build id (if any), the
/// new `CPV` and the layout switch.
struct AllocateBinpkgArgs<'a> {
    new_cp: &'a (String, String),
    new_pf: &'a str,
    suffix: &'static str,
    build_id: Option<&'a str>,
    new_cpv: &'a str,
    multi_instance: bool,
}

/// One fresh `Manifest` record: member basename, size and the checksum
/// pair.
type ManifestRecord = (String, u64, [(&'static str, String); 2]);

/// One indexed binpkg: its `CPV`, its path relative to `$PKGDIR` and the
/// `REPO` stanza value (`""` when the stanza has none; the file's own
/// `repository` wins wherever Portage reads the live database).
#[derive(Clone)]
struct BinpkgEntry {
    cpv: String,
    rel: String,
    repo: String,
}

/// The loaded `Packages` index: the header plus the package stanzas in
/// file order. Every `inject` mutates the stanzas and rewrites the file,
/// like Portage's per-`inject` `_pkgindex_write`.
pub(crate) struct BinpkgState {
    pkgdir: PathBuf,
    header: HashMap<String, String>,
    stanzas: Vec<HashMap<String, String>>,
}

impl BinpkgState {
    /// Load the index, translating the on-disk `MTIME`/`REPO`/`DESC`
    /// names back to the internal ones and dropping stanza keys outside
    /// the allowed set. A missing file is an empty index; an
    /// unsupported `VERSION` resets the header.
    // Portage: lib/portage/getbinpkg.py, PackageIndex.read
    pub(crate) fn load(pkgdir: &Path) -> Result<Self, String> {
        let path = pkgdir.join("Packages");
        let (mut header, stanzas) = match std::fs::read_to_string(&path) {
            Ok(text) => parse_packages_index(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (HashMap::new(), Vec::new()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        if !pkgindex_version_supported(&header) {
            header = HashMap::new();
        }
        let mut kept = Vec::new();
        for mut stanza in stanzas {
            if stanza.get("CPV").is_none_or(|cpv| cpv.is_empty()) {
                continue;
            }
            stanza.retain(|k, _| is_allowed_index_key(k));
            translate_stanza_to_internal(&mut stanza);
            kept.push(stanza);
        }
        Ok(Self {
            pkgdir: pkgdir.to_path_buf(),
            header,
            stanzas: kept,
        })
    }

    /// Every indexed `CPV`, for the sorted `update_ents` walk.
    fn sorted_cpvs(&self) -> Vec<String> {
        self.stanzas
            .iter()
            .filter_map(|stanza| stanza.get("CPV").cloned())
            .collect()
    }

    /// `bintree._allocate_filename` / `_allocate_filename_multi`: the new
    /// file's home. Single-instance keeps `<pkgdir>/<cpv>.<suffix>`; the
    /// multi-instance layout prefers `<cp>/<pf>-<build_id>.<suffix>` and
    /// otherwise increments past every occupied id.
    // Portage: lib/portage/dbapi/bintree.py, getname
    fn allocate_binpkg_path(&self, args: AllocateBinpkgArgs<'_>) -> PathBuf {
        let AllocateBinpkgArgs {
            new_cp,
            new_pf,
            suffix,
            build_id,
            new_cpv,
            multi_instance,
        } = args;
        let pkgdir = &self.pkgdir;
        if !multi_instance {
            return pkgdir.join(format!("{new_cpv}.{suffix}"));
        }
        if let Some(bid) = build_id.filter(|b| !b.is_empty()) {
            let preferred = pkgdir.join(format!(
                "{}/{}/{}-{bid}.{suffix}",
                new_cp.0, new_cp.1, new_pf
            ));
            if !preferred.exists() {
                return preferred;
            }
        }
        let mut build_id = self.max_build_id(new_cpv) + 1;
        loop {
            let candidate = pkgdir.join(format!(
                "{}/{}/{}-{build_id}.{suffix}",
                new_cp.0, new_cp.1, new_pf
            ));
            if !candidate.exists() {
                return candidate;
            }
            build_id += 1;
        }
    }

    /// The stanza behind a `CPV`.
    fn entry(&self, cpv: &str) -> Option<BinpkgEntry> {
        self.stanzas
            .iter()
            .find(|s| s.get("CPV").is_some_and(|c| c == cpv))
            .map(|stanza| BinpkgEntry {
                cpv: cpv.to_string(),
                rel: stanza
                    .get("PATH")
                    .cloned()
                    .unwrap_or_else(|| format!("{cpv}.tbz2")),
                repo: stanza
                    .get("REPO")
                    .or_else(|| stanza.get("repository"))
                    .cloned()
                    .unwrap_or_default(),
            })
    }

    /// Whether any stanza carries this `CPV`.
    fn has_cpv(&self, cpv: &str) -> bool {
        self.stanzas
            .iter()
            .any(|s| s.get("CPV").is_some_and(|c| c == cpv))
    }

    /// The stanza `CPV`s on one `cat/pkg`.
    fn cpvs_on_cp(&self, cp: &(String, String)) -> Vec<String> {
        self.stanzas
            .iter()
            .filter_map(|s| s.get("CPV").cloned())
            .filter(|cpv| {
                portage_repo::split_cpv(cpv).is_some_and(|(cat, pkg, _)| cat == cp.0 && pkg == cp.1)
            })
            .collect()
    }

    /// The largest numeric `BUILD_ID` on one `CPV`, for the multi-instance
    /// allocator. Entries without one do not count.
    fn max_build_id(&self, cpv: &str) -> u64 {
        self.stanzas
            .iter()
            .filter(|s| s.get("CPV").is_some_and(|c| c == cpv))
            .filter_map(|s| s.get("BUILD_ID").and_then(|b| b.parse::<u64>().ok()))
            .max()
            .unwrap_or(0)
    }

    /// Replace the stanza for one file: drop every stanza with the same
    /// `PATH` (or the same `CPV` at the same `PATH`) and append the fresh
    /// one, then rewrite the index file.
    // Portage: lib/portage/dbapi/bintree.py, _inject_file
    fn inject(
        &mut self,
        stanza: HashMap<String, String>,
        writer: &WriterConfig<'_>,
    ) -> Result<(), String> {
        let path = stanza.get("PATH").cloned().unwrap_or_default();
        let cpv = stanza.get("CPV").cloned().unwrap_or_default();
        self.stanzas.retain(|other| {
            if !path.is_empty() && other.get("PATH").is_some_and(|p| *p == path) {
                return false;
            }
            if other.get("CPV").is_some_and(|c| *c == cpv)
                && other.get("PATH").unwrap_or(&String::new()) == &path
            {
                return false;
            }
            true
        });
        self.stanzas.push(stanza);
        write_binpkg_index(&self.pkgdir, &mut self.header, &self.stanzas, writer)
    }

    /// Drop one file's stanza (the signed-gpkg removal) and rewrite the
    /// index file.
    // Portage: lib/portage/dbapi/bintree.py, remove
    fn remove(&mut self, cpv: &str, rel: &str, writer: &WriterConfig<'_>) -> Result<(), String> {
        self.stanzas.retain(|other| {
            !(other.get("CPV").is_some_and(|c| c == cpv)
                && other.get("PATH").is_some_and(|p| p == rel))
        });
        write_binpkg_index(&self.pkgdir, &mut self.header, &self.stanzas, writer)
    }
}

/// `bintree.move_ent`: every indexed binpkg on `old` whose file's
/// `repository` matches moves to `new` — the metadata rewritten, `PF` /
/// `CATEGORY` reset, the ebuild renamed — via a `.partial` copy that is
/// injected into place. The old file stays behind for `eclean-pkg`.
/// Returns the move count, which includes entries skipped after counting
/// (an occupied destination); the count lands before the fallible file
/// work, like Portage's.
// Portage: lib/portage/dbapi/bintree.py, move_ent
fn move_binpkg(
    state: &mut BinpkgState,
    old: &(String, String),
    new: &(String, String),
    repo_match: &dyn Fn(&str) -> bool,
    writer: &WriterConfig<'_>,
) -> Result<usize, String> {
    let mut moves = 0usize;
    let mut targets = state.cpvs_on_cp(old);
    targets.sort_by(|a, b| cmp_cpv(a, b));
    for cpv in targets {
        let entry = state.entry(&cpv);
        let Some(entry) = entry else {
            continue;
        };
        let meta = match read_file_meta(&state.pkgdir, &entry) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if !repo_match(&meta.repository().unwrap_or_else(|| entry.repo.clone())) {
            continue;
        }
        let Some((_, pkg, ver)) = portage_repo::split_cpv(&entry.cpv) else {
            continue;
        };
        let new_cpv = entry.cpv.replacen(
            &format!("{}/{}", old.0, old.1),
            &format!("{}/{}", new.0, new.1),
            1,
        );
        let Some((_, new_pf)) = new_cpv.split_once('/') else {
            return Err(format!("{new_cpv}: not a cat/pf"));
        };
        let pf = format!("{pkg}-{ver}");
        let build_time = meta.value("BUILD_TIME").unwrap_or("").to_string();
        if state
            .stanzas
            .iter()
            .filter(|s| s.get("CPV").is_some_and(|c| c == &new_cpv))
            .any(|s| s.get("BUILD_TIME").is_some_and(|b| b == &build_time))
        {
            continue;
        }
        if state.has_cpv(&new_cpv) {
            continue;
        }
        let full_path = state.pkgdir.join(&entry.rel);
        if std::fs::metadata(&full_path).is_err() {
            continue;
        }
        if !crate::privileges::path_writable(&full_path) {
            continue;
        }
        if meta.is_signed() {
            continue;
        }
        moves += 1;
        let mut pairs = meta.ordered_pairs();
        apply_move_to_pairs(&mut pairs, old, new);
        set_pair(&mut pairs, "PF", format!("{new_pf}\n").as_bytes());
        set_pair(&mut pairs, "CATEGORY", format!("{}\n", new.0).as_bytes());
        if new_pf != pf {
            rename_pair(
                &mut pairs,
                &format!("{pf}.ebuild"),
                &format!("{new_pf}.ebuild"),
            );
        }
        let allocated = state.allocate_binpkg_path(AllocateBinpkgArgs {
            new_cp: new,
            new_pf,
            suffix: meta.format.suffix(),
            build_id: meta.value("BUILD_ID"),
            new_cpv: &new_cpv,
            multi_instance: writer.multi_instance,
        });
        let update_path = PathBuf::from(format!("{}.partial", allocated.display()));
        write_moved_binpkg(
            WriteMovedArgs {
                full_path: &full_path,
                update_path: &update_path,
                allocated: &allocated,
                pairs: &pairs,
                meta: &meta,
                basename: new_pf_for_basename(&new_cpv, meta.format, None),
            },
            writer,
        )?;
        state.inject(
            stanza_for_file(&state.pkgdir, &allocated, &new_cpv)?,
            writer,
        )?;
    }
    Ok(moves)
}

/// `dbapi.move_slot_ent` over the binpkgs: every indexed binpkg on `cp`
/// whose file `SLOT`'s slot part equals `old_slot` gets `SLOT` rewritten
/// in place. A versioned atom first narrows the files through the shared
/// matcher, so only the matching versions move. The shared `newslot`
/// carries a sub-slot suffix across entries, like Portage's loop
/// reassignment.
// Portage: lib/portage/dbapi/__init__.py, move_slot_ent
fn move_binpkg_slot(
    state: &mut BinpkgState,
    args: MoveSlotArgs<'_>,
    repo_match: &dyn Fn(&str) -> bool,
    writer: &WriterConfig<'_>,
) -> Result<usize, String> {
    let MoveSlotArgs {
        cp,
        old_slot,
        new_slot,
        atom,
    } = args;
    let mut cpvs = state.cpvs_on_cp(cp);
    if let Some(atom) = atom {
        let keep: std::collections::HashSet<String> =
            portage_repo::slotmove_version_matches(atom, &cpvs)
                .into_iter()
                .collect();
        cpvs.retain(|cpv| keep.contains(cpv));
    }
    cpvs.sort();
    let mut newslot = new_slot.to_string();
    let mut moves = 0usize;
    for cpv in cpvs {
        let entry = state.entry(&cpv);
        let Some(entry) = entry else {
            continue;
        };
        let meta = match read_file_meta(&state.pkgdir, &entry) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        let slot = meta.value("SLOT").unwrap_or("").to_string();
        if slot.split('/').next().unwrap_or("") != old_slot {
            continue;
        }
        if !repo_match(&meta.repository().unwrap_or_else(|| entry.repo.clone())) {
            continue;
        }
        moves += 1;
        let slot_part = slot.split('/').next().unwrap_or("");
        let sub_slot = slot.split('/').nth(1).unwrap_or("");
        if !newslot.contains('/')
            && !sub_slot.is_empty()
            && sub_slot != slot_part
            && sub_slot != newslot
        {
            newslot = format!("{newslot}/{sub_slot}");
        }
        aux_update(
            state,
            &entry,
            &[("SLOT".to_string(), format!("{newslot}\n"))],
            writer,
        )?;
    }
    Ok(moves)
}

/// `binarytree.aux_update`: rewrite one file's metadata in place through
/// a same-directory temporary renamed over, then re-inject the entry. A
/// signed gpkg is removed instead (file and stanza), never rewritten.
// Portage: lib/portage/dbapi/bintree.py, aux_update
fn aux_update(
    state: &mut BinpkgState,
    entry: &BinpkgEntry,
    values: &[(String, String)],
    writer: &WriterConfig<'_>,
) -> Result<(), String> {
    let full_path = state.pkgdir.join(&entry.rel);
    let meta = read_file_meta(&state.pkgdir, entry)?;
    match meta.format {
        BinpkgFormat::Xpak => {
            let mut pairs = meta.ordered_pairs();
            for (key, value) in values {
                set_pair(&mut pairs, key, value.as_bytes());
            }
            pairs.retain(|(_, v)| !v.is_empty());
            recompose_mem(&full_path, &xpak_mem(&pairs))?;
        }
        BinpkgFormat::Gpkg => {
            if meta.is_signed() {
                std::fs::remove_file(&full_path)
                    .map_err(|e| format!("{}: {e}", full_path.display()))?;
                state.remove(&entry.cpv, &entry.rel, writer)?;
                return Ok(());
            }
            let mut pairs = meta.ordered_pairs();
            for (key, value) in values {
                set_pair(&mut pairs, key, value.as_bytes());
            }
            pairs.retain(|(_, v)| !v.is_empty());
            let basename = new_pf_for_basename(&entry.cpv, meta.format, meta.value("BUILD_ID"));
            rewrite_gpkg_file(&full_path, &basename, &pairs, &meta.gpkg, writer)?;
        }
    }
    state.inject(
        stanza_for_file(&state.pkgdir, &full_path, &entry.cpv)?,
        writer,
    )
}

/// The moved file's write-out: copy the old file to `<allocated>.partial`
/// under its lock, rewrite the partial, then inject (rename into place).
/// The gpkg container prefix becomes the new `pf`; the xpak keeps the
/// copied image with a fresh trailer.
// Portage: lib/portage/dbapi/bintree.py, move_ent
fn write_moved_binpkg(args: WriteMovedArgs<'_>, writer: &WriterConfig<'_>) -> Result<(), String> {
    let WriteMovedArgs {
        full_path,
        update_path,
        allocated,
        pairs,
        meta,
        basename,
    } = args;
    if let Some(parent) = update_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let _lock = PortageLockfile::acquire_new_lockfile(update_path)?;
    std::fs::copy(full_path, update_path)
        .map_err(|e| format!("{} -> {}: {e}", full_path.display(), update_path.display()))?;
    match meta.format {
        BinpkgFormat::Xpak => {
            recompose_mem(update_path, &xpak_mem(pairs))?;
        }
        BinpkgFormat::Gpkg => {
            rewrite_gpkg_file(update_path, &basename, pairs, &meta.gpkg, writer)?;
        }
    }
    if let Some(parent) = allocated.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::rename(update_path, allocated)
        .map_err(|e| format!("{} -> {}: {e}", update_path.display(), allocated.display()))?;
    Ok(())
}

/// The gpkg container prefix for a rewritten file: the plain `pf` for a
/// moved gpkg, the `pf` plus its `BUILD_ID` for an in-place `aux_update`
/// (the underscored `cpv_str` the call site builds the `gpkg` object
/// with).
// Portage: lib/portage/gpkg.py, gpkg / lib/portage/dbapi/bintree.py, aux_update
fn new_pf_for_basename(cpv: &str, format: BinpkgFormat, build_id: Option<&str>) -> String {
    if !matches!(format, BinpkgFormat::Gpkg) {
        return String::new();
    }
    let pf = portage_repo::split_cpv(cpv)
        .map(|(_, pkg, ver)| format!("{pkg}-{ver}"))
        .unwrap_or_default();
    match build_id.filter(|b| !b.is_empty()) {
        Some(bid) => format!("{pf}-{bid}"),
        None => pf,
    }
}

/// One `move` applied to every metadata value, like `update_dbentries`
/// over the single command (rewrites land in place, so order is kept).
// Portage: lib/portage/update.py, update_dbentries
fn apply_move_to_pairs(
    pairs: &mut [(Vec<u8>, Vec<u8>)],
    old: &(String, String),
    new: &(String, String),
) {
    let cmd = UpdateCmd::Move {
        old: old.clone(),
        new: new.clone(),
    };
    let cmds = [cmd];
    for (key, value) in pairs.iter_mut() {
        if key == b"CONTENTS" || key == b"environment.bz2" {
            continue;
        }
        let text = String::from_utf8_lossy(value);
        if let Some(new_text) = portage_repo::apply_cmds_to_dep_string(&text, &cmds) {
            *value = new_text.into_bytes();
        }
    }
}

/// Replace (or append) one metadata pair, keeping first-appearance order
/// for existing keys.
fn set_pair(pairs: &mut Vec<(Vec<u8>, Vec<u8>)>, key: &str, value: &[u8]) {
    match pairs.iter_mut().find(|(k, _)| k == key.as_bytes()) {
        Some((_, v)) => *v = value.to_vec(),
        None => pairs.push((key.as_bytes().to_vec(), value.to_vec())),
    }
}

/// Rename one metadata pair, dropping a colliding target first and
/// moving the renamed pair to the end, like `dict.pop` + reinsert.
fn rename_pair(pairs: &mut Vec<(Vec<u8>, Vec<u8>)>, from: &str, to: &str) {
    pairs.retain(|(k, _)| k != to.as_bytes());
    if let Some(pos) = pairs.iter().position(|(k, _)| k == from.as_bytes()) {
        let (_, value) = pairs.remove(pos);
        pairs.push((to.as_bytes().to_vec(), value));
    }
}

/// The two binary formats, told apart by file extension.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BinpkgFormat {
    Xpak,
    Gpkg,
}

impl BinpkgFormat {
    /// The allocated file's suffix: `xpak` for both `.tbz2` and `.xpak`,
    /// `gpkg.tar` for gpkg.
    // Portage: lib/portage/dbapi/bintree.py, _allocate_filename_multi
    fn suffix(self) -> &'static str {
        match self {
            BinpkgFormat::Xpak => "xpak",
            BinpkgFormat::Gpkg => "gpkg.tar",
        }
    }

    /// Tell the format from the file name, like `get_binpkg_format`.
    // Portage: lib/portage/dbapi/bintree.py, get_binpkg_format
    fn of(rel: &str) -> Option<Self> {
        if rel.ends_with(".gpkg.tar") {
            Some(BinpkgFormat::Gpkg)
        } else if rel.ends_with(".tbz2") || rel.ends_with(".xpak") {
            Some(BinpkgFormat::Xpak)
        } else {
            None
        }
    }
}

/// One binpkg file's metadata: the collapsed values (the `aux_get`
/// shape) plus the ordered raw pairs (the `get_data` / `unpack_metadata`
/// shape) and, for gpkg, the container parts the rewrite reuses.
// Portage: lib/portage/dbapi/bintree.py, _read_metadata
struct FileMeta {
    format: BinpkgFormat,
    values: HashMap<String, String>,
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    gpkg: GpkgParts,
}

/// The gpkg container pieces a metadata rewrite carries over: every
/// non-metadata member's header (renamed on write; bytes stream through
/// during the rebuild, never all held) plus the old `Manifest` order for
/// the surviving records.
#[derive(Default)]
struct GpkgParts {
    members: Vec<GpkgKeptMember>,
    manifest_order: Vec<String>,
    manifest_records: Vec<(String, String, String)>,
    signed: bool,
}

/// One carried-over gpkg member: its basename and its container path for
/// re-opening (bytes stream through during the rebuild, never held).
struct GpkgKeptMember {
    name: String,
    raw_name: Vec<u8>,
}

impl FileMeta {
    /// The collapsed `repository` value, `None` when the file has none.
    fn repository(&self) -> Option<String> {
        self.values.get("repository").cloned()
    }

    /// Whether the file is signature-protected: any `.sig` member or a
    /// PGP-armored `Manifest`.
    // Portage: lib/portage/gpkg.py, _verify_binpkg
    fn is_signed(&self) -> bool {
        self.gpkg.signed
    }

    /// One collapsed value by internal key.
    fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// The ordered raw pairs the rewrite folds new values into.
    fn ordered_pairs(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.pairs.clone()
    }
}

/// Read one indexed file's metadata in both shapes, verifying an
/// unsigned gpkg's digests first. An unreadable or unparseable file is an
/// error (the caller skips the entry, like an entry missing from the
/// populated database).
// Portage: lib/portage/dbapi/bintree.py, _read_metadata
fn read_file_meta(pkgdir: &Path, entry: &BinpkgEntry) -> Result<FileMeta, String> {
    let full_path = pkgdir.join(&entry.rel);
    let format = BinpkgFormat::of(&entry.rel)
        .ok_or_else(|| format!("{}: unknown binary package format", full_path.display()))?;
    match format {
        BinpkgFormat::Xpak => {
            let seg = read_xpak_segment(&full_path).map_err(|e| e.to_string())?;
            let pairs: Vec<(Vec<u8>, Vec<u8>)> = parse_xpak_members(&seg)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|(k, v)| (k.into_bytes(), v.to_vec()))
                .collect();
            let values = pairs
                .iter()
                .map(|(k, v)| {
                    (
                        String::from_utf8_lossy(k).into_owned(),
                        collapse_ws(&String::from_utf8_lossy(v)),
                    )
                })
                .collect();
            Ok(FileMeta {
                format,
                values,
                pairs,
                gpkg: GpkgParts::default(),
            })
        }
        BinpkgFormat::Gpkg => {
            let (members, _) = read_outer_members(&full_path).map_err(|e| e.to_string())?;
            let signed = gpkg_signed(&full_path, &members)?;
            if !signed {
                verify_gpkg_manifest(&full_path, &crate::binpkg::GpgVerify::from_features(""))
                    .map_err(|e| e.to_string())?;
            }
            let raw = read_gpkg_raw(&full_path, &members)?;
            let values = raw
                .pairs
                .iter()
                .map(|(k, v)| (k.clone(), collapse_ws(&String::from_utf8_lossy(v))))
                .collect();
            Ok(FileMeta {
                format,
                values,
                pairs: raw
                    .pairs
                    .iter()
                    .map(|(k, v)| (k.as_bytes().to_vec(), v.clone()))
                    .collect(),
                gpkg: GpkgParts {
                    members: raw.kept,
                    manifest_order: raw.manifest_order,
                    manifest_records: raw.manifest_records,
                    signed,
                },
            })
        }
    }
}

/// The raw gpkg pieces: the ordered metadata pairs plus every
/// non-metadata member and the old `Manifest` record order.
struct GpkgRaw {
    pairs: Vec<(String, Vec<u8>)>,
    kept: Vec<GpkgKeptMember>,
    manifest_order: Vec<String>,
    manifest_records: Vec<(String, String, String)>,
}

/// Read the metadata member plus every carried-over member and the old
/// `Manifest` order.
// Portage: lib/portage/gpkg.py, unpack_metadata
fn read_gpkg_raw(gpkg: &Path, members: &[OuterMember]) -> Result<GpkgRaw, String> {
    let metadata_member = members
        .iter()
        .find(|m| classify_inner_member("metadata", &m.name).is_some())
        .ok_or_else(|| format!("{}: no `metadata.tar` member in the gpkg", gpkg.display()))?;
    let comp = classify_inner_member("metadata", &metadata_member.name)
        .ok_or_else(|| format!("{}: no `metadata.tar` member in the gpkg", gpkg.display()))?;
    let member_bytes = outer_member_read(gpkg, &metadata_member.raw_name, |entry| {
        read_member_bytes_capped(entry, MAX_INNER_METADATA_BYTES, &metadata_member.name)
    })
    .map_err(|e| e.to_string())?;
    let inner = read_inner_metadata(gpkg, &metadata_member.name, &member_bytes, comp)
        .map_err(|e| e.to_string())?;
    let pairs = inner
        .entries
        .into_iter()
        .map(|entry| (entry.key, entry.bytes))
        .collect();
    let mut kept = Vec::new();
    for member in members {
        if member.name == "Manifest"
            || member.name == "gpkg-1"
            || classify_inner_member("metadata", &member.name).is_some()
        {
            continue;
        }
        let present = outer_member_read(gpkg, &member.raw_name, |entry| {
            Ok::<_, crate::binpkg::BinpkgError>(entry.header().entry_type().is_file())
        })
        .map_err(|e| e.to_string())?;
        if !present {
            return Err(format!(
                "{}: gpkg container member {:?} is not a regular file",
                gpkg.display(),
                member.name
            ));
        }
        kept.push(GpkgKeptMember {
            name: member.name.clone(),
            raw_name: member.raw_name.clone(),
        });
    }
    let (manifest_order, manifest_records) =
        outer_member_read(gpkg, &manifest_raw_name(members)?, |mut entry| {
            let mut text = Vec::new();
            entry.read_to_end(&mut text).map_err(|e| {
                crate::binpkg::BinpkgError::Fatal(format!("{}: {e}", gpkg.display()))
            })?;
            Ok::<_, crate::binpkg::BinpkgError>((
                manifest_data_order(&text),
                manifest_data_records(&text),
            ))
        })
        .map_err(|e| e.to_string())?;
    Ok(GpkgRaw {
        pairs,
        kept,
        manifest_order,
        manifest_records,
    })
}

/// The `Manifest` member's raw name.
fn manifest_raw_name(members: &[OuterMember]) -> Result<Vec<u8>, String> {
    members
        .iter()
        .find(|m| m.name == "Manifest")
        .map(|m| m.raw_name.clone())
        .ok_or_else(|| "gpkg container has no Manifest".to_string())
}

/// The old `Manifest`'s `DATA` basenames in order, for the surviving
/// records.
fn manifest_data_order(manifest: &[u8]) -> Vec<String> {
    manifest_data_records(manifest)
        .into_iter()
        .map(|(name, _, _)| name)
        .collect()
}

/// The old `Manifest`'s `DATA` records: basename, size and the checksum
/// words, so carried-over members keep their record verbatim.
fn manifest_data_records(manifest: &[u8]) -> Vec<(String, String, String)> {
    String::from_utf8_lossy(manifest)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            if parts.next() != Some("DATA") {
                return None;
            }
            let name = parts.next()?.to_string();
            let size = parts.next()?.to_string();
            Some((name, size, parts.collect::<Vec<_>>().join(" ")))
        })
        .collect()
}

/// Whether the gpkg carries any signature: a `.sig` sidecar or a
/// PGP-armored `Manifest`.
// Portage: lib/portage/gpkg.py, _verify_binpkg
fn gpkg_signed(gpkg: &Path, members: &[OuterMember]) -> Result<bool, String> {
    if members.iter().any(|m| m.name.ends_with(".sig")) {
        return Ok(true);
    }
    let raw = manifest_raw_name(members)?;
    let bytes = outer_member_read(gpkg, &raw, |entry| {
        crate::binpkg::read_member_bytes(entry).map_err(|e| e.to_string())
    })
    .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).contains("-----BEGIN PGP SIGNATURE-----"))
}

/// `gpkg.update_metadata`: rebuild the container with new metadata —
/// fresh `gpkg-1`, recompressed `metadata.tar`, every other member
/// carried over byte-identical under the new prefix, and a recomputed
/// `Manifest` — through a same-directory temporary renamed into place.
// Portage: lib/portage/gpkg.py, update_metadata
fn rewrite_gpkg_file(
    path: &Path,
    basename: &str,
    pairs: &[(Vec<u8>, Vec<u8>)],
    kept: &GpkgParts,
    writer: &WriterConfig<'_>,
) -> Result<(), String> {
    let now = writer.now as i64;
    let inner = build_metadata_tar(pairs, now)?;
    let (member_name, compressed) = compress_metadata(&inner, writer)?;
    let tmp = staged_temp_path(path)?;
    let result = (|| {
        let file = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        set_file_mode(&tmp, 0o644)?;
        let mut builder = tar::Builder::new(file);
        append_data_member(&mut builder, &format!("{basename}/gpkg-1"), now, &[])?;
        append_data_member(
            &mut builder,
            &format!("{basename}/{member_name}"),
            now,
            &compressed,
        )?;
        let records: Vec<ManifestRecord> = vec![
            ("gpkg-1".to_string(), 0, digest_of(&[])),
            (
                member_name.clone(),
                compressed.len() as u64,
                digest_of(&compressed),
            ),
        ];
        let by_name: HashMap<&str, &GpkgKeptMember> =
            kept.members.iter().map(|m| (m.name.as_str(), m)).collect();
        let old_by_name: HashMap<&str, &(String, String, String)> = kept
            .manifest_records
            .iter()
            .map(|record| (record.0.as_str(), record))
            .collect();
        let mut kept_lines: Vec<String> = Vec::new();
        for name in &kept.manifest_order {
            if name == "gpkg-1" || name.starts_with("metadata") || name == "Manifest" {
                continue;
            }
            let Some(member) = by_name.get(name.as_str()) else {
                continue;
            };
            outer_member_read(path, &member.raw_name, |mut entry| {
                let mut header = entry.header().clone();
                header
                    .set_path(format!("{basename}/{}", member.name))
                    .map_err(|e| {
                        crate::binpkg::BinpkgError::Fatal(format!("{}: {e}", path.display()))
                    })?;
                // `append`, unlike `append_data`, trusts the header as-is:
                // the rename changed the name the old checksum covers.
                header.set_cksum();
                builder.append(&header, &mut entry).map_err(|e| {
                    crate::binpkg::BinpkgError::Fatal(format!("{}: {e}", path.display()))
                })?;
                Ok::<_, crate::binpkg::BinpkgError>(())
            })
            .map_err(|e| e.to_string())?;
            // The bytes are unchanged, so the old record — down to its
            // checksum order — rides along verbatim.
            if let Some((_, size, hashes)) = old_by_name.get(name.as_str()) {
                kept_lines.push(format!("DATA {name} {size} {hashes}\n"));
            }
        }
        let manifest = manifest_bytes(&records, &kept_lines);
        append_data_member(
            &mut builder,
            &format!("{basename}/Manifest"),
            now,
            &manifest,
        )?;
        let mut file = builder
            .into_inner()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        file.write_all(&[0u8; 1024])
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        drop(file);
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("{} -> {}: {e}", tmp.display(), path.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    Ok(())
}

/// Build the inner `metadata.tar` (USTAR, one `metadata/<key>` member per
/// pair, fresh mtimes), like `TarFile(mode="w|", format=USTAR_FORMAT)`
/// with a `datetime.now()` mtime per member.
// Portage: lib/portage/gpkg.py, _add_metadata
fn build_metadata_tar(pairs: &[(Vec<u8>, Vec<u8>)], now: i64) -> Result<Vec<u8>, String> {
    let mut builder = tar::Builder::new(Vec::new());
    for (key, value) in pairs {
        let name = format!("metadata/{}", String::from_utf8_lossy(key));
        let mut header = tar::Header::new_ustar();
        header.set_size(value.len() as u64);
        header.set_mtime(now as u64);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        builder
            .append_data(&mut header, name, &value[..])
            .map_err(|e| format!("metadata.tar: {e}"))?;
    }
    let mut out = builder
        .into_inner()
        .map_err(|e| format!("metadata.tar: {e}"))?;
    out.extend_from_slice(&[0u8; 1024]);
    Ok(out)
}

/// Compress the inner tar with the resolved `BINPKG_COMPRESS` command
/// (name, flags, `{JOBS}` from the writer halves), or carry it raw when
/// compression is unset. The member name follows the compressor.
// Portage: lib/portage/gpkg.py, _create_tarinfo
fn compress_metadata(data: &[u8], writer: &WriterConfig<'_>) -> Result<(String, Vec<u8>), String> {
    let Some(compression) = writer.binpkg_compress else {
        return Ok(("metadata.tar".to_string(), data.to_vec()));
    };
    let jobs = makeopts_to_job_count(writer.makeopts.unwrap_or("1"));
    let argv = compression_command_argv(CompressionArgvArgs {
        compression,
        flags: writer.binpkg_compress_flags.unwrap_or(""),
        bzip2_command: writer.portage_bzip2_command.unwrap_or("bzip2"),
        jobs: &jobs,
    })?;
    let ext = match compression {
        "bzip2" => ".bz2",
        "gzip" => ".gz",
        "lz4" => ".lz4",
        "lzip" => ".lz",
        "lzop" => ".lzo",
        "xz" => ".xz",
        "zstd" => ".zst",
        _ => {
            return Err(format!(
                "portage.exception.InvalidCompressionMethod: {compression}"
            ));
        }
    };
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", argv[0]))?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    stdin
        .write_all(data)
        .map_err(|e| format!("{}: {e}", argv[0]))?;
    drop(stdin);
    let out = child
        .wait_with_output()
        .map_err(|e| format!("waiting for {}: {e}", argv[0]))?;
    if !out.status.success() {
        return Err(format!(
            "{} failed to compress metadata ({})",
            argv[0], out.status
        ));
    }
    Ok((format!("metadata.tar{ext}"), out.stdout))
}

/// The fresh `Manifest`: recomputed `DATA` lines for the version marker
/// and the metadata member, then every carried-over member's old record
/// verbatim (same bytes, same checksums, same algorithm order).
// Portage: lib/portage/gpkg.py, _add_manifest
fn manifest_bytes(records: &[ManifestRecord], kept_lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, size, digests) in records {
        out.extend_from_slice(
            format!(
                "DATA {name} {size} {} {} {} {}\n",
                digests[0].0, digests[0].1, digests[1].0, digests[1].1
            )
            .as_bytes(),
        );
    }
    for line in kept_lines {
        out.extend_from_slice(line.as_bytes());
    }
    out
}

/// Append one in-memory member with a fresh USTAR header.
fn append_data_member(
    builder: &mut tar::Builder<std::fs::File>,
    name: &str,
    mtime: i64,
    data: &[u8],
) -> Result<(), String> {
    let mut header = tar::Header::new_ustar();
    header.set_size(data.len() as u64);
    header.set_mtime(mtime as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    builder
        .append_data(&mut header, name, data)
        .map_err(|e| format!("{name}: {e}"))?;
    Ok(())
}

/// A same-directory temporary path for an atomic rewrite.
fn staged_temp_path(path: &Path) -> Result<PathBuf, String> {
    let base = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: bad file name", path.display()))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for n in 0u32.. {
        let candidate = parent.join(format!(".{base}.tmp{}.{n}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => {
                std::fs::remove_file(&candidate)
                    .map_err(|e| format!("{}: {e}", candidate.display()))?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", candidate.display())),
        }
    }
    unreachable!()
}

/// `os.fchmod(path, 0o644)`: the rewrite temp keeps Portage's mode, not
/// the umask's.
fn set_file_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// `bintree._pkgindex_entry` for a rewritten file: the collapsed aux
/// keys (`EAPI` defaulting to `"0"`), fresh MD5/SHA1 over the file, and
/// the `CPV` / `_mtime_` / `SIZE` / `PATH` bookkeeping.
// Portage: lib/portage/dbapi/bintree.py, _pkgindex_entry
fn stanza_for_file(
    pkgdir: &Path,
    full_path: &Path,
    cpv: &str,
) -> Result<HashMap<String, String>, String> {
    let values = collapsed_file_values(full_path)?;
    let bytes = std::fs::read(full_path).map_err(|e| format!("{}: {e}", full_path.display()))?;
    let st = std::fs::metadata(full_path).map_err(|e| format!("{}: {e}", full_path.display()))?;
    let mut stanza = HashMap::new();
    for key in AUX_KEYS {
        let value = values.get(*key).cloned().unwrap_or_default();
        stanza.insert(
            (*key).to_string(),
            if value.is_empty() && *key == "EAPI" {
                "0".to_string()
            } else {
                value
            },
        );
    }
    if let Some(repo_revisions) = values.get("REPO_REVISIONS") {
        stanza.insert("REPO_REVISIONS".to_string(), repo_revisions.clone());
    }
    let md5 = bytes_hex_digest(&bytes, "MD5").unwrap_or_default();
    let sha1 = bytes_hex_digest(&bytes, "SHA1").unwrap_or_default();
    stanza.insert("MD5".to_string(), md5);
    stanza.insert("SHA1".to_string(), sha1);
    stanza.insert("CPV".to_string(), cpv.to_string());
    stanza.insert("_mtime_".to_string(), file_mtime(&st).to_string());
    stanza.insert("SIZE".to_string(), bytes.len().to_string());
    let rel = full_path
        .strip_prefix(pkgdir)
        .map_err(|e| format!("{}: {e}", full_path.display()))?
        .to_string_lossy()
        .into_owned();
    stanza.insert("PATH".to_string(), rel);
    Ok(stanza)
}

/// The collapsed aux values of a rewritten file (xpak trailer or gpkg
/// metadata member), like `_read_metadata`.
fn collapsed_file_values(full_path: &Path) -> Result<HashMap<String, String>, String> {
    let name = full_path.to_string_lossy().into_owned();
    if name.ends_with(".gpkg.tar") {
        let map = read_gpkg_metadata(full_path).map_err(|e| e.to_string())?;
        return Ok(map);
    }
    let seg = read_xpak_segment(full_path).map_err(|e| e.to_string())?;
    parse_xpak_members(&seg)
        .map_err(|e| e.to_string())
        .map(|members| {
            members
                .into_iter()
                .map(|(k, v)| (k, collapse_ws(&String::from_utf8_lossy(v))))
                .collect()
        })
}

/// `PackageIndex.write` + `_pkgindex_write` for the moved tree: every
/// stanza sorted by `CPV`, falsy and default-valued fields dropped,
/// header keys inherited from the stored header with a fresh
/// `TIMESTAMP`/`PACKAGES`/`VERSION`, then the `Packages.gz` twin.
/// File mtimes follow `TIMESTAMP`.
// Portage: lib/portage/getbinpkg.py, PackageIndex.write
fn write_binpkg_index(
    pkgdir: &Path,
    header: &mut HashMap<String, String>,
    stanzas: &[HashMap<String, String>],
    writer: &WriterConfig<'_>,
) -> Result<(), String> {
    let path = pkgdir.join("Packages");
    std::fs::create_dir_all(pkgdir).map_err(|e| format!("{}: {e}", pkgdir.display()))?;
    let _lock = PortageLockfile::acquire_new_lockfile(&path)?;
    header.insert("TIMESTAMP".to_string(), writer.now.to_string());
    header.insert("PACKAGES".to_string(), stanzas.len().to_string());
    header
        .entry("VERSION".to_string())
        .or_insert_with(|| "0".to_string());
    // Unknown header keys are dropped, like the header-keyed
    // `_update_pkgindex_header` outcome for an unconfigured profile.
    let mut header_fields: Vec<(&str, &str)> = header
        .iter()
        .filter(|(key, value)| {
            !value.is_empty() && matches!(key.as_str(), "PACKAGES" | "TIMESTAMP" | "VERSION")
        })
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    header_fields.sort_by(|a, b| a.0.cmp(b.0));
    let mut ordered: Vec<&HashMap<String, String>> = stanzas.iter().collect();
    ordered.sort_by(|a, b| {
        cmp_cpv(
            a.get("CPV").map(String::as_str).unwrap_or(""),
            b.get("CPV").map(String::as_str).unwrap_or(""),
        )
    });
    let mut blocks: Vec<String> = Vec::new();
    for stanza in ordered {
        let mut fields: Vec<(&str, &str)> = Vec::new();
        for (key, value) in stanza.iter() {
            let key = key.as_str();
            if value.is_empty() || !(is_allowed_index_key(key) || key == "REPO_REVISIONS") {
                continue;
            }
            if omit_stanza_default(key, value).is_empty() {
                continue;
            }
            if (key == "CHOST" || key == "repository")
                && header.get(key).is_some_and(|h| !h.is_empty() && h == value)
            {
                continue;
            }
            let display = match key {
                "_mtime_" => "MTIME",
                "repository" => "REPO",
                "DESCRIPTION" => "DESC",
                _ => key,
            };
            fields.push((display, value.as_str()));
        }
        sort_stanza_fields_for_write(&mut fields);
        blocks.push(format_packages_entry(&fields));
    }
    let mut text = String::new();
    for (key, value) in &header_fields {
        text.push_str(&format!("{key}: {value}\n"));
    }
    text.push('\n');
    for block in blocks {
        text.push_str(block.trim());
        text.push_str("\n\n");
    }
    std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    if writer.compress_index {
        write_packages_gz(pkgdir, text.as_bytes(), writer.now)?;
    } else {
        let gz = pkgdir.join("Packages.gz");
        match std::fs::remove_file(&gz) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", gz.display())),
        }
    }
    set_mtime_to(&path, writer.now)?;
    if writer.compress_index {
        set_mtime_to(&pkgdir.join("Packages.gz"), writer.now)?;
    }
    Ok(())
}

/// Set a file's mtime to the index `TIMESTAMP` (`os.utime` also sets the
/// atime, which has no reader and stays untouched here).
fn set_mtime_to(path: &Path, now: u64) -> Result<(), String> {
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(now);
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(time))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// CPV ordering for the index write: category, package name, then
/// `vercmp` on the version.
// Portage: lib/portage/getbinpkg.py, PackageIndex.write
fn cmp_cpv(a: &str, b: &str) -> std::cmp::Ordering {
    let (a_cat, a_pf) = a.split_once('/').unwrap_or((a, ""));
    let (b_cat, b_pf) = b.split_once('/').unwrap_or((b, ""));
    (
        a_cat,
        portage_repo::split_pf(a_pf)
            .map(|(pn, _)| pn)
            .unwrap_or_default(),
    )
        .cmp(&(
            b_cat,
            portage_repo::split_pf(b_pf)
                .map(|(pn, _)| pn)
                .unwrap_or_default(),
        ))
        .then_with(|| {
            let a_ver = portage_repo::split_pf(a_pf)
                .map(|(_, v)| v)
                .unwrap_or_default();
            let b_ver = portage_repo::split_pf(b_pf)
                .map(|(_, v)| v)
                .unwrap_or_default();
            portage_versions::vercmp(&a_ver, &b_ver)
                .map(|o| o.cmp(&0))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// The stanza keys `PackageIndex.read` keeps: every key a freshly-built
/// entry can carry. Fresh entries bypass this (their `REPO_REVISIONS`
/// rides along, like `_inject_file`'s unfiltered append).
// Portage: lib/portage/dbapi/bintree.py, _pkgindex_allowed_pkg_keys
fn is_allowed_index_key(key: &str) -> bool {
    matches!(
        key,
        "BASE_URI"
            | "BDEPEND"
            | "BUILD_ID"
            | "BUILD_TIME"
            | "CHOST"
            | "CPV"
            | "DEFINED_PHASES"
            | "DEPEND"
            | "DESC"
            | "DESCRIPTION"
            | "EAPI"
            | "FETCHCOMMAND"
            | "IDEPEND"
            | "IUSE"
            | "KEYWORDS"
            | "LICENSE"
            | "MD5"
            | "MTIME"
            | "PATH"
            | "PDEPEND"
            | "PKGINDEX_URI"
            | "PROPERTIES"
            | "PROVIDES"
            | "RDEPEND"
            | "REPO"
            | "REQUIRES"
            | "RESTRICT"
            | "RESUMECOMMAND"
            | "SHA1"
            | "SIZE"
            | "SLOT"
            | "USE"
            | "USER_PATCHES"
            | "_mtime_"
            | "repository"
    )
}

/// Translate the on-disk `MTIME`/`REPO`/`DESC` names back to the internal
/// ones on load.
// Portage: lib/portage/getbinpkg.py, PackageIndex._read_translation_map
fn translate_stanza_to_internal(stanza: &mut HashMap<String, String>) {
    for (display, internal) in [
        ("MTIME", "_mtime_"),
        ("REPO", "repository"),
        ("DESC", "DESCRIPTION"),
    ] {
        if let Some(value) = stanza.remove(display) {
            stanza.insert(internal.to_string(), value);
        }
    }
}

/// Whether the loaded header's `VERSION` is still supported: present and
/// not newer than the writer's.
// Portage: lib/portage/dbapi/bintree.py, _pkgindex_version_supported
fn pkgindex_version_supported(header: &HashMap<String, String>) -> bool {
    header
        .get("VERSION")
        .and_then(|v| v.parse::<i64>().ok())
        .is_some_and(|v| v <= 0)
}

/// `os.access(first_existing(pkgdir), W_OK)`: the nearest existing
/// ancestor decides, like Portage's `bindb.writable` gate.
fn pkgdir_writable(pkgdir: &Path) -> bool {
    let mut current = Some(pkgdir);
    while let Some(path) = current {
        match std::fs::symlink_metadata(path) {
            Ok(_) => return crate::privileges::path_writable(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                current = path.parent();
            }
            Err(_) => return crate::privileges::path_writable(path),
        }
    }
    false
}

/// Clock reading for the index `TIMESTAMP` and the fresh tar members.
pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The aux-cache keys a stanza is built from, plus `CPV`/`SIZE`.
// Portage: lib/portage/dbapi/bintree.py, _pkgindex_keys
const AUX_KEYS: &[&str] = &[
    "BDEPEND",
    "BUILD_ID",
    "BUILD_TIME",
    "CHOST",
    "DEFINED_PHASES",
    "DEPEND",
    "EAPI",
    "IDEPEND",
    "IUSE",
    "KEYWORDS",
    "LICENSE",
    "MD5",
    "PDEPEND",
    "PROPERTIES",
    "PROVIDES",
    "RDEPEND",
    "repository",
    "REQUIRES",
    "RESTRICT",
    "SIZE",
    "SLOT",
    "USE",
    "USER_PATCHES",
    "_mtime_",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::global_updates::repo_update_map;
    use std::collections::BTreeMap;

    /// One normalized `Packages` stanza: sorted `(key, value)` fields.
    type IndexBlock = Vec<(String, String)>;

    /// Portage 3.0.83 copies the moved xpak aside (`dev-libs/newpkg/`
    /// under `binpkg-multi-instance`, `make.globals:78`), rewrites the
    /// moved/slotted metadata in place, and refreshes `Packages` on every
    /// inject (`bintree.py:639-831` for the move, `:449-494` for the
    /// slot, `dbapi/__init__.py:383-447` for `update_ents`,
    /// `bintree.py:1895-2011,2237` for `inject`/`_pkgindex_write`). The
    /// committed capture is the oracle: xpak files byte for byte, the
    /// gpkg by decoded metadata, `Packages` by text fields.
    #[test]
    fn binpkg_files_and_index_match_the_portage_capture() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture = manifest.join("../../fixtures/global-updates");
        let capture = manifest.join("../../LLM/evidence/2026-10-25-review/t5/after");
        let scratch = portage_util::TempDir::new("global_updates_binpkg");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        for (from, to) in [
            (&fixture.join("root"), &root),
            (&fixture.join("repo"), &repo),
        ] {
            let ok = std::process::Command::new("cp")
                .args(["-a", &from.to_string_lossy(), &to.to_string_lossy()])
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "cp -a {} failed", from.display());
        }
        let repos = test_repos(&repo);
        let map = repo_update_map(&repos);
        let pkgdir = root.join("pkgdir");
        let report = apply_binpkg_moves(BinpkgMovesArgs {
            pkgdir: &pkgdir,
            repos: &repos,
            map: &map,
            multi_instance: true,
            binpkg_compress: Some("zstd"),
            binpkg_compress_flags: None,
            portage_bzip2_command: None,
            makeopts: None,
            compress_index: true,
            on_progress: None,
        })
        .expect("binpkg moves apply");
        assert_eq!(report.moves, 1, "one binpkg move_ent copy");
        assert_eq!(report.slot_moves, 1, "one binpkg move_slot_ent rewrite");
        let calls = std::cell::RefCell::new(Vec::new());
        let updated = update_binpkg_ents(UpdateBinpkgEntsArgs {
            pkgdir: &pkgdir,
            repos: &repos,
            map: &map,
            binpkg_compress: Some("zstd"),
            binpkg_compress_flags: None,
            portage_bzip2_command: None,
            makeopts: None,
            compress_index: true,
            on_update: Some(&|maxval, curval| {
                calls.borrow_mut().push((maxval, curval));
            }),
        })
        .expect("binpkg dep strings rewrite");
        assert_eq!(updated, 3, "oldpkg, newpkg and slotpkg change");
        assert_eq!(
            *calls.borrow(),
            vec![(3, 0), (3, 1), (3, 2), (3, 3)],
            "the progress hook fires once per rewritten binpkg"
        );

        let got = tree_files(&pkgdir);
        let after = tree_files(&capture.join("pkgdir"));
        assert_eq!(
            got.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>(),
            "the pkgdir layout matches the capture"
        );
        // The xpak trailer rewrite is byte-exact (same member order,
        // same value bytes), so these compare verbatim; only the gpkg
        // container (fresh mtimes, fresh compression) cannot.
        for rel in [
            "dev-libs/oldpkg-1.0.tbz2",
            "dev-libs/newpkg/newpkg-1.0-1.xpak",
        ] {
            assert_eq!(
                got.get(rel),
                after.get(rel),
                "{rel}: bytes differ from the capture"
            );
        }
        // The old file stays behind for `eclean-pkg`; the rewrite leaves
        // no `.partial` or lock files around.
        for (rel, bytes) in &got {
            assert!(
                !rel.contains(".partial") && !rel.contains("portage_lockfile"),
                "{rel}: leftover from the rewrite"
            );
            let _ = bytes;
        }
        // Gpkg by decoded metadata: member values, Manifest shape and the
        // image member's digest (which proves its bytes are untouched).
        let ours = read_gpkg_metadata(&pkgdir.join("dev-libs/slotpkg-1.0.gpkg.tar"))
            .expect("rewritten gpkg reads");
        let theirs = read_gpkg_metadata(&capture.join("pkgdir/dev-libs/slotpkg-1.0.gpkg.tar"))
            .expect("capture gpkg reads");
        let ours_map: HashMap<String, String> = ours.into_iter().collect();
        let theirs_map: HashMap<String, String> = theirs.into_iter().collect();
        assert_eq!(ours_map, theirs_map, "gpkg metadata members match");
        let ours_manifest = manifest_lines(&pkgdir.join("dev-libs/slotpkg-1.0.gpkg.tar"));
        let theirs_manifest = manifest_lines(&capture.join("pkgdir/dev-libs/slotpkg-1.0.gpkg.tar"));
        assert_eq!(
            manifest_names(&ours_manifest),
            manifest_names(&theirs_manifest),
            "Manifest member set and order match"
        );
        assert_eq!(
            manifest_line(&ours_manifest, "image.tar.zst"),
            manifest_line(&theirs_manifest, "image.tar.zst"),
            "the image member is byte-identical (same size and digests)"
        );
        verify_gpkg_manifest(
            &pkgdir.join("dev-libs/slotpkg-1.0.gpkg.tar"),
            &crate::binpkg::GpgVerify::from_features(""),
        )
        .expect("rewritten gpkg verifies");
        // Packages by text fields: TIMESTAMP/MTIME always refresh, and
        // the gpkg row's SIZE/digests follow its fresh container bytes.
        let (ours_header, ours_stanzas) = normalized_index(
            &std::fs::read_to_string(pkgdir.join("Packages")).unwrap(),
            true,
        );
        let (theirs_header, theirs_stanzas) = normalized_index(
            &std::fs::read_to_string(capture.join("pkgdir/Packages")).unwrap(),
            true,
        );
        assert_eq!(ours_header, theirs_header, "Packages header matches");
        assert_eq!(ours_stanzas, theirs_stanzas, "Packages stanzas match");
        // Packages.gz carries the same bytes it was written from.
        let gz = std::fs::read(pkgdir.join("Packages.gz")).unwrap();
        let mut decoder = flate2::read::GzDecoder::new(&gz[..]);
        let mut plain = Vec::new();
        decoder.read_to_end(&mut plain).unwrap();
        assert_eq!(
            plain,
            std::fs::read(pkgdir.join("Packages")).unwrap(),
            "Packages.gz matches Packages"
        );
        // Everything outside pkgdir is untouched by this piece.
        assert_eq!(
            std::fs::read(root.join("var/lib/portage/world")).unwrap(),
            std::fs::read(fixture.join("root/var/lib/portage/world")).unwrap(),
            "world is byte-identical to the fixture"
        );
        assert_eq!(
            tree_files(&root.join("var/db/pkg")),
            tree_files(&fixture.join("root/var/db/pkg")),
            "vdb is byte-identical to the fixture"
        );
    }

    /// Portage 3.0.83 skips the whole binpkg half when `PKGDIR` is not
    /// writable (`_do_global_updates` sees `bindb` as `None`,
    /// `_global_updates.py:141-145`): a read-only `pkgdir` reports zero
    /// moves and leaves every file and the index alone.
    #[test]
    fn read_only_pkgdir_skips_binpkg_updates() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture = manifest.join("../../fixtures/global-updates");
        let scratch = portage_util::TempDir::new("global_updates_binpkg_ro");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        for (from, to) in [
            (&fixture.join("root"), &root),
            (&fixture.join("repo"), &repo),
        ] {
            let ok = std::process::Command::new("cp")
                .args(["-a", &from.to_string_lossy(), &to.to_string_lossy()])
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "cp -a {} failed", from.display());
        }
        let repos = test_repos(&repo);
        let map = repo_update_map(&repos);
        let pkgdir = root.join("pkgdir");
        set_mode(&pkgdir, 0o555);
        let report = apply_binpkg_moves(BinpkgMovesArgs {
            pkgdir: &pkgdir,
            repos: &repos,
            map: &map,
            multi_instance: true,
            binpkg_compress: Some("zstd"),
            binpkg_compress_flags: None,
            portage_bzip2_command: None,
            makeopts: None,
            compress_index: true,
            on_progress: None,
        })
        .expect("read-only moves report");
        assert_eq!(report.moves, 0, "no binpkg move without a writable pkgdir");
        assert_eq!(
            report.slot_moves, 0,
            "no binpkg slotmove without a writable pkgdir"
        );
        let updated = update_binpkg_ents(UpdateBinpkgEntsArgs {
            pkgdir: &pkgdir,
            repos: &repos,
            map: &map,
            binpkg_compress: Some("zstd"),
            binpkg_compress_flags: None,
            portage_bzip2_command: None,
            makeopts: None,
            compress_index: true,
            on_update: None,
        })
        .expect("read-only update_ents reports");
        assert_eq!(
            updated, 0,
            "no binpkg update_ents without a writable pkgdir"
        );
        assert_eq!(
            tree_files(&pkgdir),
            tree_files(&fixture.join("root/pkgdir")),
            "a read-only pkgdir is byte-identical to the fixture"
        );
        set_mode(&pkgdir, 0o755);
    }

    /// Portage 3.0.83 never rewrites a signed gpkg: `aux_update` removes
    /// the file and its index entry instead (`bintree.py:286-297`). A
    /// scratch `.sig` sidecar plus its `Manifest` record trips the same
    /// branch here: the slotpkg file goes away and its stanza with it,
    /// while the unsigned xpak still rewrites.
    #[test]
    fn signed_gpkg_is_removed_not_rewritten() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture = manifest.join("../../fixtures/global-updates");
        let scratch = portage_util::TempDir::new("global_updates_binpkg_signed");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        for (from, to) in [
            (&fixture.join("root"), &root),
            (&fixture.join("repo"), &repo),
        ] {
            let ok = std::process::Command::new("cp")
                .args(["-a", &from.to_string_lossy(), &to.to_string_lossy()])
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "cp -a {} failed", from.display());
        }
        sign_gpkg(&root.join("pkgdir/dev-libs/slotpkg-1.0.gpkg.tar"));
        let repos = test_repos(&repo);
        let map = repo_update_map(&repos);
        let pkgdir = root.join("pkgdir");
        let calls = std::cell::RefCell::new(Vec::new());
        let updated = update_binpkg_ents(UpdateBinpkgEntsArgs {
            pkgdir: &pkgdir,
            repos: &repos,
            map: &map,
            binpkg_compress: Some("zstd"),
            binpkg_compress_flags: None,
            portage_bzip2_command: None,
            makeopts: None,
            compress_index: true,
            on_update: Some(&|maxval, curval| {
                calls.borrow_mut().push((maxval, curval));
            }),
        })
        .expect("signed update_ents applies");
        assert_eq!(updated, 2, "oldpkg rewrites, slotpkg is removed");
        assert_eq!(
            *calls.borrow(),
            vec![(2, 0), (2, 1), (2, 2)],
            "the removal still reports progress"
        );
        assert!(
            !pkgdir.join("dev-libs/slotpkg-1.0.gpkg.tar").exists(),
            "the signed gpkg is gone"
        );
        let index = std::fs::read_to_string(pkgdir.join("Packages")).unwrap();
        let (_, stanzas) = portage_repo::parse_packages_index(&index);
        assert!(
            stanzas
                .iter()
                .all(|s| s.get("CPV") != Some(&"dev-libs/slotpkg-1.0".to_string())),
            "the signed gpkg's stanza is gone"
        );
        assert!(
            index.contains("dev-libs/oldpkg-1.0"),
            "the unsigned xpak's stanza stays"
        );
    }

    /// Portage 3.0.83 skips a signed gpkg in `move_ent` but leaves the
    /// file alone (`bintree.py:653-663`): with only a move for the signed
    /// package, nothing moves and the file stays.
    #[test]
    fn signed_gpkg_move_is_skipped() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture = manifest.join("../../fixtures/global-updates");
        let scratch = portage_util::TempDir::new("global_updates_binpkg_signmove");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        for (from, to) in [
            (&fixture.join("root"), &root),
            (&fixture.join("repo"), &repo),
        ] {
            let ok = std::process::Command::new("cp")
                .args(["-a", &from.to_string_lossy(), &to.to_string_lossy()])
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "cp -a {} failed", from.display());
        }
        sign_gpkg(&root.join("pkgdir/dev-libs/slotpkg-1.0.gpkg.tar"));
        std::fs::write(
            repo.join("profiles/updates/1Q-2026"),
            "move dev-libs/slotpkg dev-libs/renamed\n",
        )
        .unwrap();
        let repos = test_repos(&repo);
        let map = repo_update_map(&repos);
        let pkgdir = root.join("pkgdir");
        let report = apply_binpkg_moves(BinpkgMovesArgs {
            pkgdir: &pkgdir,
            repos: &repos,
            map: &map,
            multi_instance: true,
            binpkg_compress: Some("zstd"),
            binpkg_compress_flags: None,
            portage_bzip2_command: None,
            makeopts: None,
            compress_index: true,
            on_progress: None,
        })
        .expect("signed moves report");
        assert_eq!(report.moves, 0, "the signed gpkg does not move");
        assert!(
            pkgdir.join("dev-libs/slotpkg-1.0.gpkg.tar").is_file(),
            "the signed gpkg stays behind"
        );
    }

    /// A versioned `slotmove` reslots only the matching binpkgs: with
    /// `slotpkg-1.0` and `slotpkg-2.0` both carrying `SLOT` 0,
    /// `slotmove >=dev-libs/slotpkg-2 0 1` rewrites only 2.0 in place.
    /// Portage matches `atom.with_slot(origslot)` against the indexed
    /// cpvs (`lib/portage/dbapi/__init__.py:449-494` via `bindb`); the S0
    /// probe shows `>=dev-libs/foo-2` keeping only `foo-2`.
    #[test]
    fn versioned_slotmove_reslots_only_the_matching_binpkg() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture = manifest.join("../../fixtures/global-updates");
        let scratch = portage_util::TempDir::new("global_updates_binpkg_versionslot");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        for (from, to) in [
            (&fixture.join("root"), &root),
            (&fixture.join("repo"), &repo),
        ] {
            let ok = std::process::Command::new("cp")
                .args(["-a", &from.to_string_lossy(), &to.to_string_lossy()])
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "cp -a {} failed", from.display());
        }
        // A second indexed version beside the fixture's `slotpkg-1.0`.
        let pkgdir = root.join("pkgdir");
        std::fs::copy(
            pkgdir.join("dev-libs/slotpkg-1.0.gpkg.tar"),
            pkgdir.join("dev-libs/slotpkg-2.0.gpkg.tar"),
        )
        .unwrap();
        let index = pkgdir.join("Packages");
        let text = std::fs::read_to_string(&index).unwrap();
        let stanza = text
            .split("\n\n")
            .find(|block| block.contains("CPV: dev-libs/slotpkg-1.0"))
            .expect("fixture indexes slotpkg-1.0")
            .replace("slotpkg-1.0", "slotpkg-2.0");
        std::fs::write(&index, format!("{text}\n{stanza}\n")).unwrap();
        let repos = test_repos(&repo);
        let map = vec![(
            "gutest".to_string(),
            vec![UpdateCmd::SlotMove {
                cp: ("dev-libs".to_string(), "slotpkg".to_string()),
                old_slot: "0".to_string(),
                new_slot: "1".to_string(),
                atom: Some(">=dev-libs/slotpkg-2".to_string()),
            }],
        )];
        let report = apply_binpkg_moves(BinpkgMovesArgs {
            pkgdir: &pkgdir,
            repos: &repos,
            map: &map,
            multi_instance: true,
            binpkg_compress: Some("zstd"),
            binpkg_compress_flags: None,
            portage_bzip2_command: None,
            makeopts: None,
            compress_index: true,
            on_progress: None,
        })
        .expect("versioned binpkg slotmove applies");
        assert_eq!(report.moves, 0, "no binpkg move_ent copy");
        assert_eq!(report.slot_moves, 1, "only the matching binpkg moves");
        let slot_of = |rel: &str| {
            read_gpkg_metadata(&pkgdir.join(rel))
                .expect("rewritten gpkg reads")
                .into_iter()
                .find(|pair| pair.0 == "SLOT")
                .map(|pair| pair.1)
                .expect("gpkg carries SLOT")
        };
        assert_eq!(
            slot_of("dev-libs/slotpkg-2.0.gpkg.tar"),
            "1",
            "slotpkg-2.0 is reslotted"
        );
        assert_eq!(
            slot_of("dev-libs/slotpkg-1.0.gpkg.tar"),
            "0",
            "slotpkg-1.0 stays behind"
        );
    }

    /// The one `gutest` main repo the fixture's updates live in.
    fn test_repos(repo: &Path) -> Vec<RepoConfig> {
        vec![RepoConfig {
            name: "gutest".to_string(),
            location: repo.to_path_buf(),
            priority: 0,
            is_main: true,
            masters: Vec::new(),
            profile_formats: Vec::new(),
            cache_formats: Vec::new(),
            aliases: Vec::new(),
            sync_type: None,
            sync_uri: None,
            volatile: false,
            module_specific_options: Vec::new(),
        }]
    }

    /// Every file under `dir` as `relative/path -> bytes`.
    fn tree_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(top) = stack.pop() {
            for entry in std::fs::read_dir(&top).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path
                        .strip_prefix(dir)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned();
                    out.insert(rel, std::fs::read(&path).unwrap());
                }
            }
        }
        out
    }

    /// Parse a `Packages` file into its header block and ordered stanza
    /// blocks, normalizing what the oracle cannot pin: `TIMESTAMP` and
    /// `MTIME` always refresh, and the gpkg row's `SIZE`/digests follow
    /// its fresh container bytes (fresh member mtimes, fresh
    /// compression). Xpak rows stay byte-exact, so their digests pin.
    fn normalized_index(
        text: &str,
        gpkg_sizes_float: bool,
    ) -> (Vec<(String, String)>, Vec<IndexBlock>) {
        let (header, stanzas) = portage_repo::parse_packages_index(text);
        let mut header_block: Vec<(String, String)> = header
            .into_iter()
            .filter(|(k, _)| k != "TIMESTAMP")
            .collect();
        header_block.sort();
        let stanzas = stanzas
            .into_iter()
            .map(|mut stanza| {
                stanza.remove("MTIME");
                if gpkg_sizes_float
                    && stanza
                        .get("CPV")
                        .is_some_and(|c| c == "dev-libs/slotpkg-1.0")
                {
                    stanza.remove("SIZE");
                    stanza.remove("MD5");
                    stanza.remove("SHA1");
                }
                let mut block: Vec<(String, String)> = stanza.into_iter().collect();
                block.sort();
                block
            })
            .collect();
        (header_block, stanzas)
    }

    /// One gpkg's `Manifest` as `(name, size, hashes)` lines.
    fn manifest_lines(gpkg: &Path) -> Vec<(String, u64, String)> {
        let file = std::fs::File::open(gpkg).unwrap();
        let mut archive = tar::Archive::new(file);
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let raw = entry.path_bytes().to_vec();
            if raw.ends_with(b"/Manifest") {
                let mut text = String::new();
                entry.read_to_string(&mut text).unwrap();
                return text
                    .lines()
                    .filter_map(|line| {
                        let mut parts = line.split_whitespace();
                        if parts.next()? != "DATA" {
                            return None;
                        }
                        Some((
                            parts.next()?.to_string(),
                            parts.next()?.parse().ok()?,
                            parts.collect::<Vec<_>>().join(" "),
                        ))
                    })
                    .collect();
            }
        }
        panic!("{}: no Manifest member", gpkg.display());
    }

    /// The `DATA` line for one member.
    fn manifest_line(lines: &[(String, u64, String)], name: &str) -> (u64, String) {
        lines
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, size, hashes)| (*size, hashes.clone()))
            .unwrap_or_else(|| panic!("no Manifest line for {name}"))
    }

    /// Just the member names, in `Manifest` order.
    fn manifest_names(lines: &[(String, u64, String)]) -> Vec<String> {
        lines.iter().map(|(n, _, _)| n.clone()).collect()
    }

    /// Craft a signed gpkg out of the fixture one: a fake
    /// `image.tar.zst.sig` sidecar plus its `Manifest` record, so the
    /// signature branch trips the way Portage's `_verify_binpkg` trips
    /// it (any `.sig` member).
    fn sign_gpkg(path: &Path) {
        let prefix = "slotpkg-1.0";
        let sig_name = "image.tar.zst.sig";
        let sig_bytes = b"fake-signature\n";
        let (_, _, hashes) = {
            let [(_, blake), (_, sha)] = digest_of(sig_bytes);
            (
                sha.clone(),
                blake.clone(),
                format!("SHA512 {sha} BLAKE2B {blake}"),
            )
        };
        let file = std::fs::File::open(path).unwrap();
        let mut archive = tar::Archive::new(file);
        let mut members: Vec<(Vec<u8>, tar::Header, Vec<u8>)> = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let raw = entry.path_bytes().to_vec();
            let header = entry.header().clone();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            members.push((raw, header, data));
        }
        let tmp = path.with_extension("tar.signing");
        let out = std::fs::File::create(&tmp).unwrap();
        let mut builder = tar::Builder::new(out);
        for (raw, mut header, data) in members {
            let mut text;
            let bytes = if raw.ends_with(b"/Manifest") {
                text = String::from_utf8(data).unwrap();
                text.push_str(&format!("DATA {sig_name} {} {hashes}\n", sig_bytes.len()));
                text.into_bytes()
            } else {
                data
            };
            header.set_size(bytes.len() as u64);
            header.set_cksum();
            builder.append(&header, &bytes[..]).unwrap();
        }
        {
            let mut header = tar::Header::new_ustar();
            header.set_size(sig_bytes.len() as u64);
            header.set_mtime(now_secs());
            header.set_mode(0o644);
            builder
                .append_data(&mut header, format!("{prefix}/{sig_name}"), &sig_bytes[..])
                .unwrap();
        }
        let mut out = builder.into_inner().unwrap();
        out.write_all(&[0u8; 1024]).unwrap();
        drop(out);
        std::fs::rename(&tmp, path).unwrap();
    }

    /// `chmod` a path.
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
}
