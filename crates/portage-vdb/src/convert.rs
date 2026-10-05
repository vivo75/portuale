//! Generic copy and comparison between two [`InstalledDb`]s.
//!
//! [`copy_all`] writes the whole copy in **one** destination transaction,
//! so a failed copy leaves a database destination unchanged (a failed
//! `files` copy is partial; the CLI says so). Only one [`EntryImage`] is
//! in memory at a time.
//!
//! [`verify`] compares the two backends **directly** through
//! [`InstalledDb::entry_image`], which already normalises both sides, so
//! no temporary copy is needed. The round trip files -> sqlite -> files is
//! still checked by running `verify` on the two ends.

use crate::types::DIR_MODE_MASK;
use std::collections::BTreeMap;

use crate::{
    ConfigMemory, Counter, EntryFile, EntryImage, EntryKey, InstalledDb, MetadataStamp, Result,
};

/// What [`copy_all`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyReport {
    /// Live entries written to the destination, in order.
    pub copied: Vec<EntryKey>,
    /// Source entries that were mid-merge; not copied.
    pub pending_skipped: Vec<EntryKey>,
    /// Destination entries absent from the source that `force` deleted.
    pub removed: Vec<EntryKey>,
    pub world_atoms: usize,
    pub world_sets: usize,
    pub preserved_libs: usize,
    pub config_memory: usize,
    /// The counter written, `None` when the source has none.
    pub counter: Option<Counter>,
}

/// Copy every live entry and the neighbouring stores of `src` into `dst`.
///
/// `dst` must hold no live entry unless `force`; with `force` same-key
/// entries are replaced and extras deleted. Entries go in `(category, pf)`
/// order; pending (`merging`) entries are reported, not copied. The
/// counter is set last, to the source's value, never renumbered.
pub fn copy_all(src: &dyn InstalledDb, dst: &dyn InstalledDb, force: bool) -> Result<CopyReport> {
    let mut dst_keys = dst.entries()?;
    dst_keys.sort();
    if !force && !dst_keys.is_empty() {
        return Err(crate::Error::Invalid(format!(
            "destination is not empty ({} live entries, first {}); \
             refusing to copy over it (use --force to replace it)",
            dst_keys.len(),
            dst_keys[0]
        )));
    }
    let mut keys = src.entries()?;
    keys.sort();
    keys.dedup();
    let mut report = CopyReport {
        pending_skipped: src.pending_entries()?,
        ..CopyReport::default()
    };

    let world = src.world()?;
    let sets = src.world_sets()?;
    let mut libs = src.preserved_libs()?;
    libs.loaded = dst.preserved_libs()?.entries;
    let cfg: ConfigMemory = src.config_memory()?;
    let counter = src.counter()?;
    let src_generation = src.generation()?;
    let src_vdb_dir = src.vdb_dir();

    let mut txn = dst.begin_write()?;
    if src.kind() == crate::BackendKind::Files
        && dst.kind() != crate::BackendKind::Files
        && let Some(vdb_path) = src_vdb_dir
        && let Some(root) = vdb_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
    {
        txn.set_import_mark(src_generation, &root.to_string_lossy())?;
    }
    for key in &dst_keys {
        if keys.binary_search(key).is_err() {
            txn.delete_entry(key)?;
            report.removed.push(key.clone());
        }
    }
    for key in keys {
        let Some(image) = src.entry_image(&key)? else {
            // Vanished between listing and reading (live source).
            continue;
        };
        txn.insert_entry(&image)?;
        report.copied.push(key);
    }
    report.world_atoms = world.atoms.len();
    report.world_sets = sets.sets.len();
    report.preserved_libs = libs.entries.len();
    report.config_memory = cfg.entries.len();
    txn.set_world(&world)?;
    txn.set_world_sets(&sets)?;
    txn.set_preserved_libs(&libs)?;
    txn.set_config_memory(&cfg)?;
    if let Some(c) = counter {
        txn.set_counter(c)?;
        report.counter = Some(c);
    }
    txn.commit()?;
    Ok(report)
}

/// The result of [`verify`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerifyReport {
    /// Live entries compared (those present on both sides).
    pub entries_compared: usize,
    /// Every difference found.
    pub total_differences: usize,
    /// The first [`VERIFY_MAX_LISTED`] differences, one line each.
    pub differences: Vec<String>,
}

/// How many differences a [`VerifyReport`] lists (the count is exact).
pub const VERIFY_MAX_LISTED: usize = 100;

impl VerifyReport {
    /// No difference at all.
    pub fn is_equal(&self) -> bool {
        self.total_differences == 0
    }

    fn push(&mut self, line: String) {
        self.total_differences += 1;
        if self.differences.len() < VERIFY_MAX_LISTED {
            self.differences.push(line);
        }
    }
}

/// Compare `a` and `b` logically: same live entry set; per entry the same
/// file set, bytes, modes, mtimes and `metadata` stamp *state*; same
/// `world`, `world_sets`, `preserved_libs`, `config_memory` and counter.
/// A [`MetadataStamp::Valid`] stamp on both sides skips the `#dir_mtime=`
/// line (a writer restamps it); stale bytes compare exactly.
pub fn verify(a: &dyn InstalledDb, b: &dyn InstalledDb) -> Result<VerifyReport> {
    let mut rep = VerifyReport::default();
    let mut ka = a.entries()?;
    let mut kb = b.entries()?;
    ka.sort();
    kb.sort();
    for k in &ka {
        if kb.binary_search(k).is_err() {
            rep.push(format!("{k}: only in the first database"));
        }
    }
    for k in &kb {
        if ka.binary_search(k).is_err() {
            rep.push(format!("{k}: only in the second database"));
        }
    }
    for k in ka.iter().filter(|k| kb.binary_search(k).is_ok()) {
        match (a.entry_image(k)?, b.entry_image(k)?) {
            (Some(ia), Some(ib)) => {
                rep.entries_compared += 1;
                compare_images(&mut rep, &ia, &ib);
            }
            _ => rep.push(format!("{k}: vanished while verifying")),
        }
    }

    let (wa, wb) = (a.world()?, b.world()?);
    if wa != wb {
        rep.push(format!("world: {:?} != {:?}", wa.atoms, wb.atoms));
    }
    let (sa, sb) = (a.world_sets()?, b.world_sets()?);
    if sa != sb {
        rep.push(format!("world_sets: {:?} != {:?}", sa.sets, sb.sets));
    }
    let (pa, pb) = (a.preserved_libs()?.entries, b.preserved_libs()?.entries);
    if pa != pb {
        let keys: Vec<&String> = pa
            .keys()
            .chain(pb.keys())
            .filter(|k| pa.get(*k) != pb.get(*k))
            .collect();
        rep.push(format!("preserved_libs: differ at {keys:?}"));
    }
    let (ca, cb) = (a.config_memory()?.entries, b.config_memory()?.entries);
    if ca != cb {
        let keys: Vec<&String> = ca
            .keys()
            .chain(cb.keys())
            .filter(|k| ca.get(*k) != cb.get(*k))
            .collect();
        rep.push(format!("config_memory: differ at {keys:?}"));
    }
    let (na, nb) = (a.counter()?, b.counter()?);
    if na != nb {
        rep.push(format!("counter: {na:?} != {nb:?}"));
    }
    Ok(rep)
}

/// `metadata` bytes without the trailing `#dir_mtime=` line(s).
fn without_stamp(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for line in data.split_inclusive(|&c| c == b'\n') {
        if !line.starts_with(b"#dir_mtime=") {
            out.extend_from_slice(line);
        }
    }
    out
}

fn compare_images(rep: &mut VerifyReport, a: &EntryImage, b: &EntryImage) {
    let k = &a.key;
    // Permission bits only: an older converted database may hold the full
    // `st_mode` a `files` reader used to report.
    if a.dir_mode & DIR_MODE_MASK != b.dir_mode & DIR_MODE_MASK {
        rep.push(format!(
            "{k}: directory mode {:o} != {:o}",
            a.dir_mode & DIR_MODE_MASK,
            b.dir_mode & DIR_MODE_MASK
        ));
    }
    if a.dir_mtime_ns != b.dir_mtime_ns {
        rep.push(format!(
            "{k}: directory mtime {} != {}",
            a.dir_mtime_ns, b.dir_mtime_ns
        ));
    }
    if a.metadata_stamp != b.metadata_stamp {
        rep.push(format!(
            "{k}: metadata stamp state {} != {}",
            stamp_name(a.metadata_stamp),
            stamp_name(b.metadata_stamp)
        ));
    }
    let both_valid =
        a.metadata_stamp == MetadataStamp::Valid && b.metadata_stamp == a.metadata_stamp;
    let fa: BTreeMap<&[u8], &EntryFile> = a
        .files
        .iter()
        .map(|f| (f.meta.name.as_bytes(), f))
        .collect();
    let fb: BTreeMap<&[u8], &EntryFile> = b
        .files
        .iter()
        .map(|f| (f.meta.name.as_bytes(), f))
        .collect();
    for (name, x) in &fa {
        let n = &x.meta.name;
        let Some(y) = fb.get(name) else {
            rep.push(format!("{k}/{n}: only in the first database"));
            continue;
        };
        let bytes_equal = if both_valid && n == "metadata" {
            without_stamp(&x.data) == without_stamp(&y.data)
        } else {
            x.data == y.data
        };
        if !bytes_equal {
            rep.push(format!(
                "{k}/{n}: bytes differ ({} vs {} bytes)",
                x.data.len(),
                y.data.len()
            ));
        }
        if x.meta.mode != y.meta.mode {
            rep.push(format!(
                "{k}/{n}: mode {:o} != {:o}",
                x.meta.mode, y.meta.mode
            ));
        }
        if x.meta.mtime_ns != y.meta.mtime_ns {
            rep.push(format!(
                "{k}/{n}: mtime {} != {}",
                x.meta.mtime_ns, y.meta.mtime_ns
            ));
        }
    }
    for (name, y) in &fb {
        if !fa.contains_key(name) {
            rep.push(format!("{k}/{}: only in the second database", y.meta.name));
        }
    }
}

fn stamp_name(s: MetadataStamp) -> &'static str {
    match s {
        MetadataStamp::Absent => "absent",
        MetadataStamp::Valid => "valid",
        MetadataStamp::Stale => "stale",
    }
}
