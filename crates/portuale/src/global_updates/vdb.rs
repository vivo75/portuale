//! The vdb half of `_do_global_updates`: `move_ent` renames an installed
//! entry's directory, its ebuild and its `PF`/`CATEGORY`; `move_slot_ent`
//! rewrites `SLOT`; `update_ents` rewrites the `*DEPEND` files.
// Portage: lib/portage/_global_updates.py, _do_global_updates

use crate::global_updates::{RepoMatch, repo_update_map};
use portage_repo::RepoConfig;
use portage_vdb::{EntryKey, InstalledDb};
use std::path::Path;

/// What [`apply_vdb_moves`] needs: the root whose vdb moves, and the
/// repos (in `portdb` order) whose updates apply to it. The production
/// driver lands in a later piece; tests are the only callers until then.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct VdbMovesArgs<'a> {
    pub root: &'a Path,
    pub repos: &'a [RepoConfig],
}

/// How many entries [`apply_vdb_moves`] renamed (`move`) and reslotted.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct VdbMoveReport {
    pub moves: usize,
    pub slot_moves: usize,
}

/// Apply every repo's `move`/`slotmove` to the root's vdb, holding the
/// vdb lock. A non-matching `repository`, a missing entry and an
/// occupied destination skip the entry; dep strings are untouched.
///
/// # Errors
///
/// Returns `Err` when the vdb cannot be read, locked or written.
// Portage: lib/portage/_global_updates.py, _do_global_updates
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn apply_vdb_moves(args: VdbMovesArgs<'_>) -> Result<VdbMoveReport, String> {
    let db = portage_vdb::for_root(args.root);
    // `vardb.lock()`: the blocking flock on the vdb directory. Database
    // backends serialize in their own write transactions instead.
    // Portage: lib/portage/dbapi/vartree.py, lock
    let _lock = db
        .vdb_dir()
        .map(|vdb| crate::portage_lock::PortageLockfile::acquire_new_lockfile(&vdb))
        .transpose()?;
    let map = repo_update_map(args.repos);
    let master = args
        .repos
        .iter()
        .find(|r| r.is_main)
        .map(|r| r.name.clone());
    let matcher = RepoMatch::new(&map, master);
    let mut report = VdbMoveReport {
        moves: 0,
        slot_moves: 0,
    };
    for (repo_name, cmds) in &map {
        if repo_name == "DEFAULT" || cmds.is_empty() {
            continue;
        }
        let repo_match = |repository: &str| matcher.matches(repo_name, repository);
        for cmd in cmds {
            match cmd {
                portage_repo::UpdateCmd::Move { old, new } => {
                    report.moves += move_ent(&db, old, new, &repo_match)?;
                }
                portage_repo::UpdateCmd::SlotMove {
                    cp,
                    old_slot,
                    new_slot,
                } => {
                    report.slot_moves += move_slot_ent(&db, cp, old_slot, new_slot, &repo_match)?;
                }
            }
        }
    }
    Ok(report)
}

/// What [`update_ents`] needs: the root whose vdb dep strings move,
/// the repos (in `portdb` order) whose updates apply to it, and the
/// progress hook the output driver passes (`None` in tests).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct UpdateEntsArgs<'a> {
    pub root: &'a Path,
    pub repos: &'a [RepoConfig],
    pub on_update: Option<&'a dyn Fn(usize, usize)>,
}

/// `dbapi.update_ents` over the vdb: every entry's `*DEPEND` files are
/// read through the `aux_get` whitespace collapse, rewritten with the
/// entry's own repo updates, and written back when changed. Returns how
/// many entries changed.
///
/// # Errors
///
/// Returns `Err` when the vdb cannot be read or written.
// Portage: lib/portage/dbapi/__init__.py, update_ents
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn update_ents(args: UpdateEntsArgs<'_>) -> Result<usize, String> {
    let db = portage_vdb::for_root(args.root);
    let map = repo_update_map(args.repos);
    let master = args
        .repos
        .iter()
        .find(|r| r.is_main)
        .map(|r| r.name.clone());
    let matcher = RepoMatch::new(&map, master);
    let mut keys = db.entries().map_err(|e| e.to_string())?;
    keys.sort();
    let maxval = keys.len();
    if let Some(on_update) = args.on_update {
        on_update(maxval, 0);
    }
    let mut updated = 0usize;
    for (pos, key) in keys.iter().enumerate() {
        // `repo_dict[pkg.repo]`, else the main repo's list: the first
        // map entry whose updates cover this entry's repository.
        let repository = entry_repo(&db, key)?;
        let cmds: &[portage_repo::UpdateCmd] = map
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
        let mut rewritten: Vec<(&str, String)> = Vec::new();
        for &dep_key in DEP_KEYS {
            let bytes = db.read_file(key, dep_key).map_err(|e| e.to_string())?;
            if let Some(new) = portage_repo::apply_cmds_to_dep_string(&dep_content(bytes), cmds) {
                rewritten.push((dep_key, new));
            }
        }
        if rewritten.is_empty() {
            continue;
        }
        let mut txn = db.begin_write().map_err(|e| e.to_string())?;
        for (name, data) in &rewritten {
            txn.replace_file(key, name, data.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        txn.commit().map_err(|e| e.to_string())?;
        updated += 1;
        if let Some(on_update) = args.on_update {
            on_update(maxval, pos + 1);
        }
    }
    Ok(updated)
}

/// The dep keys `update_ents` rewrites, in Portage's order.
// Portage: lib/_emerge/Package.py, _dep_keys
const DEP_KEYS: &[&str] = &["BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND"];

/// `vartree._aux_get` for a dep key: a missing file reads as `""`, and
/// a present one collapses every whitespace run to one blank (the dep
/// keys never match the multi-line exception).
// Portage: lib/portage/dbapi/vartree.py, _aux_get
fn dep_content(raw: Option<Vec<u8>>) -> String {
    let text = raw
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every installed entry on `cp`, in `cat/pf` order.
fn entries_on_cp(
    db: &std::sync::Arc<dyn InstalledDb>,
    cp: &(String, String),
) -> Result<Vec<EntryKey>, String> {
    let mut keys: Vec<EntryKey> = db
        .entries()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|k| {
            k.category == cp.0 && portage_repo::split_pf(&k.pf).is_some_and(|(pn, _)| pn == cp.1)
        })
        .collect();
    keys.sort();
    Ok(keys)
}

/// The `repository` file of an entry, `""` when absent.
fn entry_repo(db: &std::sync::Arc<dyn InstalledDb>, key: &EntryKey) -> Result<String, String> {
    db.aux_get(key, "repository")
        .map_err(|e| e.to_string())
        .map(|v| v.unwrap_or_default())
}

/// `vartree.move_ent`: every installed entry on `old` moves to `new` —
/// the directory, the ebuild when the `pf` changed, `PF`/`CATEGORY` —
/// when its `repository` matches. The count includes entries whose
/// destination already exists (they stay where they are).
// Portage: lib/portage/dbapi/vartree.py, move_ent
fn move_ent(
    db: &std::sync::Arc<dyn InstalledDb>,
    old: &(String, String),
    new: &(String, String),
    repo_match: &dyn Fn(&str) -> bool,
) -> Result<usize, String> {
    let mut moves = 0usize;
    for key in entries_on_cp(db, old)? {
        if !repo_match(&entry_repo(db, &key)?) {
            continue;
        }
        // `isvalidatom(newcp, eapi=…)`: the parse-time `bare_cp` check
        // already rejected a malformed name.
        // Portage: lib/portage/dep.py, isvalidatom
        let new_cpv = format!("{}/{}", old.0, key.pf).replacen(
            &format!("{}/{}", old.0, old.1),
            &format!("{}/{}", new.0, new.1),
            1,
        );
        let Some((_, new_pf)) = new_cpv.split_once('/') else {
            return Err(format!("{new_cpv}: not a cat/pf"));
        };
        if !db.has_entry(&key).map_err(|e| e.to_string())? {
            continue;
        }
        moves += 1;
        let dest = EntryKey::new(&new.0, new_pf);
        if db.has_entry(&dest).map_err(|e| e.to_string())? {
            continue;
        }
        let Some(mut image) = db.entry_image(&key).map_err(|e| e.to_string())? else {
            continue;
        };
        image.key = dest.clone();
        if new_pf != key.pf {
            rename_in_image(
                &mut image,
                &format!("{}.ebuild", key.pf),
                &format!("{new_pf}.ebuild"),
            );
        }
        set_image_file(&mut image, "PF", format!("{new_pf}\n").as_bytes());
        set_image_file(&mut image, "CATEGORY", format!("{}\n", new.0).as_bytes());
        let mut txn = db.begin_write().map_err(|e| e.to_string())?;
        txn.insert_entry(&image).map_err(|e| e.to_string())?;
        txn.delete_entry(&key).map_err(|e| e.to_string())?;
        txn.commit().map_err(|e| e.to_string())?;
        // `delete_entry` drops an emptied category directory; Portage's
        // rename keeps it.
        // Portage: lib/portage/dbapi/vartree.py, move_ent
        if new.0 != key.category
            && let Some(vdb) = db.vdb_dir()
        {
            let cat_dir = vdb.join(&key.category);
            if !cat_dir.exists() {
                std::fs::create_dir_all(&cat_dir).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(moves)
}

/// `dbapi.move_slot_ent`: every installed entry on `cp` in `old_slot`
/// gets `SLOT` rewritten to `new_slot`, keeping a differing sub-slot.
// Portage: lib/portage/dbapi/__init__.py, move_slot_ent
fn move_slot_ent(
    db: &std::sync::Arc<dyn InstalledDb>,
    cp: &(String, String),
    old_slot: &str,
    new_slot: &str,
    repo_match: &dyn Fn(&str) -> bool,
) -> Result<usize, String> {
    let mut matched: Vec<EntryKey> = Vec::new();
    for key in entries_on_cp(db, cp)? {
        let slot = db
            .aux_get(&key, "SLOT")
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        if slot.split('/').next().unwrap_or("") == old_slot {
            matched.push(key);
        }
    }
    // Reassigned inside the loop like Portage's `newslot`, so a second
    // entry inherits the first one's sub-slot suffix.
    let mut newslot = new_slot.to_string();
    let mut moves = 0usize;
    for key in matched {
        if !repo_match(&entry_repo(db, &key)?) {
            continue;
        }
        moves += 1;
        let slot = db
            .aux_get(&key, "SLOT")
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let slot_part = slot.split('/').next().unwrap_or("");
        let sub_slot = slot.split('/').nth(1).unwrap_or("");
        if !newslot.contains('/')
            && !sub_slot.is_empty()
            && sub_slot != slot_part
            && sub_slot != newslot
        {
            newslot = format!("{newslot}/{sub_slot}");
        }
        let mut txn = db.begin_write().map_err(|e| e.to_string())?;
        txn.replace_file(&key, "SLOT", format!("{newslot}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        txn.commit().map_err(|e| e.to_string())?;
    }
    Ok(moves)
}

/// Rename a file inside an entry image, dropping a colliding target
/// first — `os.rename` overwrites just the same.
// Portage: lib/portage/dbapi/vartree.py, move_ent
fn rename_in_image(image: &mut portage_vdb::EntryImage, from: &str, to: &str) {
    image.files.retain(|f| f.meta.name != to);
    if let Some(f) = image.files.iter_mut().find(|f| f.meta.name == from) {
        f.meta.name = to.to_string();
    }
    image.files.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
}

/// Replace (or add) one file's bytes in an entry image.
fn set_image_file(image: &mut portage_vdb::EntryImage, name: &str, data: &[u8]) {
    match image.files.iter_mut().find(|f| f.meta.name == name) {
        Some(f) => {
            f.data = data.to_vec();
            f.meta.len = data.len() as u64;
        }
        None => image.files.push(portage_vdb::EntryFile {
            meta: portage_vdb::FileMeta {
                name: name.to_string(),
                len: data.len() as u64,
                mode: 0o644,
                mtime_ns: 0,
            },
            data: data.to_vec(),
        }),
    }
    image.files.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Portage 3.0.83 applies the fixture's `move` + `slotmove` by
    /// renaming `dev-libs/oldpkg-1.0` to `dev-libs/newpkg-1.0`
    /// (directory, ebuild, `PF`, `CATEGORY`) and rewriting
    /// `slotpkg-1.0/SLOT` to `1`, leaving every dep string for
    /// `update_ents` (`lib/portage/_global_updates.py:190-206`;
    /// `vartree.move_ent`, `dbapi.move_slot_ent`). The expected tree is
    /// the committed Portage capture; dep-string files are pinned to the
    /// before-state instead.
    #[test]
    fn vdb_moves_match_the_portage_capture() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture = manifest.join("../../fixtures/global-updates");
        let capture = manifest.join("../../LLM/evidence/2026-10-25-review/t5/after/var/db/pkg");
        let scratch = portage_util::TempDir::new("global_updates_vdb");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        for (from, to) in [(fixture.join("root"), &root), (fixture.join("repo"), &repo)] {
            let ok = std::process::Command::new("cp")
                .args(["-a", &from.to_string_lossy(), &to.to_string_lossy()])
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "cp -a {} failed", from.display());
        }
        let repos = vec![RepoConfig {
            name: "gutest".to_string(),
            location: repo,
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
        }];
        let report = apply_vdb_moves(VdbMovesArgs {
            root: &root,
            repos: &repos,
        })
        .expect("vdb moves apply");
        assert_eq!(report.moves, 1, "one move_ent rename");
        assert_eq!(report.slot_moves, 1, "one move_slot_ent rewrite");

        let before = tree_files(&fixture.join("root/var/db/pkg"));
        let after = tree_files(&capture);
        let got = tree_files(&root.join("var/db/pkg"));
        assert_eq!(
            got.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>(),
            "the vdb file layout matches the capture"
        );
        for (rel, bytes) in &got {
            let expected = if is_dep_file(rel) {
                // `move_ent`/`move_slot_ent` never touch dep strings;
                // the capture's rewrites there are `update_ents`.
                before
                    .get(&before_path(rel))
                    .unwrap_or_else(|| panic!("{rel}: no before-state file"))
            } else {
                after
                    .get(rel)
                    .unwrap_or_else(|| panic!("{rel}: no capture file"))
            };
            assert_eq!(bytes, expected, "{rel}: bytes differ from the pinned state");
        }
        // World is a later piece: the moves must not touch it.
        assert_eq!(
            std::fs::read(root.join("var/lib/portage/world")).unwrap(),
            std::fs::read(fixture.join("root/var/lib/portage/world")).unwrap(),
            "world is byte-identical to the fixture"
        );
    }

    /// Portage 3.0.83 rewrites the moved tree's dep strings with the
    /// entry's own repo updates (`dbapi.update_ents` over the five dep
    /// keys, `lib/portage/dbapi/__init__.py:383-447`; per-token rewrites
    /// `lib/portage/update.py:23-104`; driven after the moves with the
    /// `#` callback, `lib/portage/_global_updates.py:253-261`). The
    /// `aux_get` whitespace collapse drops the trailing newlines, so the
    /// capture's rewritten files end without one. World, `etc/portage`
    /// and the binpkgs are untouched by this piece.
    #[test]
    fn vdb_dep_strings_match_the_portage_capture() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture = manifest.join("../../fixtures/global-updates");
        let capture = manifest.join("../../LLM/evidence/2026-10-25-review/t5/after");
        let scratch = portage_util::TempDir::new("global_updates_ents");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        for (from, to) in [(fixture.join("root"), &root), (fixture.join("repo"), &repo)] {
            let ok = std::process::Command::new("cp")
                .args(["-a", &from.to_string_lossy(), &to.to_string_lossy()])
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "cp -a {} failed", from.display());
        }
        let repos = vec![RepoConfig {
            name: "gutest".to_string(),
            location: repo,
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
        }];
        apply_vdb_moves(VdbMovesArgs {
            root: &root,
            repos: &repos,
        })
        .expect("vdb moves apply");
        let calls = std::cell::RefCell::new(Vec::new());
        let updated = update_ents(UpdateEntsArgs {
            root: &root,
            repos: &repos,
            on_update: Some(&|maxval, curval| {
                calls.borrow_mut().push((maxval, curval));
            }),
        })
        .expect("dep strings rewrite");
        assert_eq!(updated, 3, "deppkg, newpkg and slotpkg change");
        assert_eq!(
            *calls.borrow(),
            vec![(3, 0), (3, 1), (3, 2), (3, 3)],
            "the progress hook fires once per rewritten entry"
        );

        let after = tree_files(&capture.join("var/db/pkg"));
        let got = tree_files(&root.join("var/db/pkg"));
        assert_eq!(
            got.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>(),
            "the vdb file layout matches the capture"
        );
        for (rel, bytes) in &got {
            assert_eq!(
                bytes,
                after
                    .get(rel)
                    .unwrap_or_else(|| panic!("{rel}: no capture file")),
                "{rel}: bytes differ from the capture"
            );
        }
        // Later pieces own everything else: world, config and binpkgs
        // stay byte-identical to the fixture.
        for rel in [
            "var/lib/portage/world",
            "etc/portage/package.use",
            "pkgdir/dev-libs/oldpkg-1.0.tbz2",
            "pkgdir/dev-libs/slotpkg-1.0.gpkg.tar",
            "pkgdir/Packages",
        ] {
            assert_eq!(
                std::fs::read(root.join(rel)).unwrap(),
                std::fs::read(fixture.join("root").join(rel)).unwrap(),
                "{rel}: untouched by update_ents"
            );
        }
    }

    /// `*DEPEND` files, rewritten by `update_ents`, not by the moves.
    fn is_dep_file(rel: &str) -> bool {
        matches!(
            rel.rsplit('/').next().unwrap_or(""),
            "DEPEND" | "RDEPEND" | "PDEPEND" | "IDEPEND" | "BDEPEND"
        )
    }

    /// Where a result-tree file lived before the moves: the renamed
    /// entry maps back to its old directory.
    fn before_path(rel: &str) -> String {
        rel.replacen("dev-libs/newpkg-1.0/", "dev-libs/oldpkg-1.0/", 1)
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
}
