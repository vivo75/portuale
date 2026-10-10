//! Record the entry's fetch restriction and the bytes left to download.

use crate::resolve::pass::*;
use portage_profile::Config;
use std::sync::Arc;

/// The values [`record_entry_fetch`] reads from its caller.
pub(crate) struct RecordEntryFetchArgs<'a> {
    pub(crate) candidate_source: CandidateSource,
    pub(crate) resolved: &'a Candidate,
    pub(crate) key: &'a (String, String),
    pub(crate) candidate_str: &'a str,
    pub(crate) config: &'a Config,
    pub(crate) state: &'a mut PassState,
    pub(crate) entry_idx: usize,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) repo_location: &'a Path,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) candidate_remote: bool,
    pub(crate) version: &'a str,
}

/// Record the entry's fetch restriction and the bytes left to download.
pub(crate) fn record_entry_fetch(args: RecordEntryFetchArgs<'_>) {
    let RecordEntryFetchArgs {
        candidate_source,
        resolved,
        key,
        candidate_str,
        config,
        state,
        entry_idx,
        metadata,
        use_flags,
        repo_location,
        ctx,
        candidate_remote,
        version,
    } = args;
    // Real `output.py:633-641`'s own `f`/`F` fetch-restrict column
    // (`not pkg.built` -> ebuild candidates only; a binary is already
    // built and never re-fetched). `resolved.restrict` is the raw
    // `RESTRICT`; `evaluated_metadata_tokens` resolves its USE
    // conditionals the same way it does for `interactive`'s
    // `PROPERTIES`. `fetch_restrict_satisfied` is real
    // `not getfetchsizes(only_restricted=True)`.
    if candidate_source == CandidateSource::Ebuild
        && evaluated_metadata_tokens(
            &resolved.restrict,
            resolved,
            &key.0,
            &key.1,
            candidate_str,
            config,
        )
        .contains("fetch")
    {
        state.entries[entry_idx].fetch_restrict = true;
        state.entries[entry_idx].fetch_restrict_satisfied = fetch_restrict_files_all_present(
            metadata
                .get("SRC_URI")
                .map(String::as_str)
                .unwrap_or_default(),
            use_flags,
            repo_location,
            &key.0,
            &key.1,
            ctx.distdir,
        );
    }

    // Real `output.py:300-332`'s own `_calc_size` -> `counters.
    // totalsize` (the `-v` `Total:` line's `Size of downloads`): the
    // bytes still to fetch for this ebuild merge, per distfile.
    // Runs for every merge-bound ebuild entry, not just
    // fetch-restricted ones. See `GraphEntry::download_files`.
    if candidate_source == CandidateSource::Ebuild {
        state.entries[entry_idx].download_files = fetch_bytes_to_download(
            metadata
                .get("SRC_URI")
                .map(String::as_str)
                .unwrap_or_default(),
            use_flags,
            repo_location,
            &key.0,
            &key.1,
            ctx.distdir,
        );
    } else if candidate_remote {
        // A `--getbinpkg` remote binary: real `_calc_size` -> real
        // `bindbapi.getfetchsizes` returns `{<cpv>: SIZE}` from the
        // binhost `Packages` index for a package not yet downloaded
        // (`bintree.isremote`). Feeds both the `-v` per-line ` N KiB`
        // suffix and the `Size of downloads:` counter. A local
        // `$PKGDIR` binary is already present -> nothing to fetch.
        let cpv = format!("{}/{}-{version}", key.0, key.1);
        let size = metadata.get("SIZE").and_then(|s| s.parse::<u64>().ok());
        if let Some(size) = size {
            state.entries[entry_idx].download_files = vec![(cpv, size)];
        }
    }
}
