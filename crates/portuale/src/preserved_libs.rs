// Real `post_emerge()`'s own preserved-libs advisory (`lib/_emerge/
// post_emerge.py:126-152`) + `display_preserved_libs()`
// (`lib/portage/util/_dyn_libs/display_preserved_libs.py`): after any
// `emerge` run that changed the vdb, if the `preserved_libs_registry`
// still has entries, print the list of preserved libraries and their
// consumers and tell the user to run `emerge @preserved-rebuild`.
//
// Real `post_emerge` reloads + prunes the registry first ("in order to
// ensure that we do not display stale data"); portuale already prunes
// during the merge/unmerge itself (`ebuild_merge::prune_unused_preserved_
// libs`), so the registry read here is already current.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::color::Colorizer;
use crate::needed_elf;

const MAX_DISPLAY: usize = 3;

/// Real `post_emerge()`'s own gate + banner + `display_preserved_libs()`
/// call + trailer. Called at the end of every `emerge` action that can
/// change the vdb (merge, `--resume`, `--unmerge`, `--depclean`,
/// `--prune`). A no-op when the registry is empty. `--quiet` collapses
/// the whole thing to a one-line `!!! existing preserved libs found`.
pub fn show_preserved_libs_notice(root: &Path, color: &Colorizer, quiet: bool, verbose: bool) {
    let plib_dict = crate::ebuild_merge::preserved_lib_paths(root);
    if plib_dict.is_empty() {
        return;
    }

    println!();
    if quiet {
        println!("{} existing preserved libs found", color.c("WARN", "!!!"));
        return;
    }
    println!("{} existing preserved libs:", color.c("WARN", "!!!"));
    display_preserved_libs(root, &plib_dict, color, verbose);
    println!(
        "Use {} to rebuild packages using these libraries",
        color.c("GOOD", "emerge @preserved-rebuild")
    );
}

/// Real `display_preserved_libs(vardb, verbose)`
/// (`display_preserved_libs.py`): the per-`cpv` `>>> package:` blocks.
fn display_preserved_libs(
    root: &Path,
    plib_dict: &BTreeMap<String, Vec<String>>,
    color: &Colorizer,
    verbose: bool,
) {
    let owner_entries = needed_elf::read_all_needed_entries(root);
    let map = needed_elf::rebuild(root, &owner_entries);
    let defpath = needed_elf::getlibpaths(root, None);

    let all_preserved: BTreeSet<String> = plib_dict.values().flatten().cloned().collect();

    // Real `consumer_map`: preserved path -> its sorted consumers, with
    // any consumer that is itself one of the *same* provider package's
    // own preserved libs filtered out (real `internal_plib_keys`).
    let mut consumer_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for paths in plib_dict.values() {
        // Real `internal_plib_keys`: a consumer that is itself one of the
        // *same* provider package's own preserved libs is filtered out.
        let internal: BTreeSet<needed_elf::ObjKey> =
            paths.iter().map(|p| needed_elf::obj_key(root, p)).collect();
        for f in paths {
            if consumer_map.contains_key(f) {
                continue;
            }
            let consumers = match needed_elf::find_consumers(root, &map, &defpath, f, None, false) {
                Ok(set) => set,
                Err(_) => {
                    let soname = f.rsplit('/').next().unwrap_or(f);
                    needed_elf::soname_consumers(&map, soname)
                }
            };
            let mut list: Vec<String> = consumers
                .into_iter()
                .filter(|c| !internal.contains(&needed_elf::obj_key(root, c)))
                .collect();
            list.sort();
            consumer_map.insert(f.clone(), list);
        }
    }

    for (cpv, paths) in plib_dict {
        println!("{} package: {cpv}", color.c("WARN", ">>>"));

        // Real `samefile_map`: group this cpv's paths by obj key so a
        // hardlink alias is listed once, under all its names.
        let mut samefile: BTreeMap<needed_elf::ObjKey, BTreeSet<String>> = BTreeMap::new();
        for f in paths {
            samefile
                .entry(needed_elf::obj_key(root, f))
                .or_default()
                .insert(f.clone());
        }

        for alt_paths in samefile.values() {
            let alt: Vec<&String> = alt_paths.iter().collect();
            for p in &alt {
                println!("{} - {p}", color.c("WARN", " * "));
            }
            let f = alt[0];
            let mut consumers = consumer_map.get(f).cloned().unwrap_or_default();
            // Real: prefer the non-preserved consumers if there are any
            // (a preserved-lib consumer is rebuilt via its own block).
            let non_preserved: Vec<String> = consumers
                .iter()
                .filter(|c| !all_preserved.contains(*c))
                .cloned()
                .collect();
            if !non_preserved.is_empty() {
                consumers = non_preserved;
            }

            let max_display = if verbose {
                consumers.len()
            } else if consumers.len() == MAX_DISPLAY + 1 {
                MAX_DISPLAY + 1
            } else {
                MAX_DISPLAY
            };

            for c in consumers.iter().take(max_display) {
                let owners_desc = if all_preserved.contains(c) {
                    "preserved".to_string()
                } else {
                    map.obj_properties
                        .get(&needed_elf::obj_key(root, c))
                        .map(|p| p.owner.clone())
                        .unwrap_or_default()
                };
                println!("{}     used by {c} ({owners_desc})", color.c("WARN", " * "));
            }
            if !verbose && consumers.len() > max_display {
                println!(
                    "{}     used by {} other files",
                    color.c("WARN", " * "),
                    consumers.len() - max_display
                );
            }
        }
    }
}
