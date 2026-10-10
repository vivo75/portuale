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
    // Ebuild candidates only: a binary is already built and never re-fetched.
    // Portage: output.py, getfetchsizes (only_restricted)
    if candidate_source == CandidateSource::Ebuild
        && evaluated_metadata_tokens(EvaluatedMetadataTokensArgs {
            value_str: &resolved.restrict,
            candidate: resolved,
            category: &key.0,
            package: &key.1,
            candidate_str,
            config,
        })
        .contains("fetch")
    {
        state.entries[entry_idx].fetch_restrict = true;
        state.entries[entry_idx].fetch_restrict_satisfied =
            fetch_restrict_files_all_present(FetchRestrictFilesAllPresentArgs {
                src_uri: metadata
                    .get("SRC_URI")
                    .map(String::as_str)
                    .unwrap_or_default(),
                use_flags,
                repo_location,
                category: &key.0,
                package: &key.1,
                distdir: ctx.distdir,
            });
    }

    // The bytes still to fetch for this ebuild merge, per distfile.
    // Portage: output.py, _calc_size. See `GraphEntry::download_files`.
    if candidate_source == CandidateSource::Ebuild {
        state.entries[entry_idx].download_files =
            fetch_bytes_to_download(FetchBytesToDownloadArgs {
                src_uri: metadata
                    .get("SRC_URI")
                    .map(String::as_str)
                    .unwrap_or_default(),
                use_flags,
                repo_location,
                category: &key.0,
                package: &key.1,
                distdir: ctx.distdir,
            });
    } else if candidate_remote {
        // A `--getbinpkg` remote binary not yet downloaded: size from the
        // binhost `Packages` index. A local `$PKGDIR` binary is already
        // present, so nothing to fetch.
        let cpv = format!("{}/{}-{version}", key.0, key.1);
        let size = metadata.get("SIZE").and_then(|s| s.parse::<u64>().ok());
        if let Some(size) = size {
            state.entries[entry_idx].download_files = vec![(cpv, size)];
        }
    }
}
