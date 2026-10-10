//! GLEP 42 news.

use crate::pretend::*;

/// Real `emerge --check-news` (`_emerge/actions.py:3844` ->
/// `portage.news.getUnreadItems` / `NewsManager`): print the `IMPORTANT:
/// N news items need reading` block if any repo's `.unread` count is
/// nonzero, else ` * No news items were found.` (unless `--quiet`).
///
/// A news item lives at `<repo>/metadata/news/<id>/<id>.<lang>.txt`
/// (`<lang>` = `en` here -- `L10N`/`PORTAGE_L10N` not modelled). It is
/// **valid** iff its `News-Item-Format:` header matches `[12].*` (real
/// `NewsItem.isValid`). It is **relevant** iff it has no `Display-If-*`
/// restriction, or each kind it carries (`Display-If-Installed` against
/// the vdb, `Display-If-Profile` against the profile path,
/// `Display-If-Keyword` against `ARCH`) has a matching value -- see
/// `news_item_relevant`.
///
/// Real `NewsManager.updateItems`'s write-back (`news.py:112-190`) is
/// shipped: an item not already in `.skip` is (re-)evaluated; once
/// valid+relevant, it's added to *both* `.unread` and `.skip`
/// (permanently -- from then on it's `.skip`-excluded from
/// re-evaluation, even if it later stops matching a live
/// `Display-If-*` restriction). The printed count is real's own `N =
/// len(.unread)` (`getUnreadItems`, `news.py:234`) -- **not** a live
/// recompute -- so an item, once counted, stays counted on every future
/// run until something removes it from `.unread`. The only thing real
/// ever does that is `eselect news read`, a separate tool portuale
/// doesn't implement as its own action; `<eroot>/var/lib/gentoo/news/
/// news-<repo>.read` models that tool's effect (an id there is removed
/// from the `unread` set, lazily, on the next `--check-news` run,
/// rather than requiring a real `eselect news read` invocation to have
/// rewritten the file directly). Either file is rewritten (sorted, one
/// id per line) only when its set actually changed --
/// `write_news_state_if_changed`. `Display-If-Installed` atoms are
/// matched in full (version operators, slot/sub-slot) -- see
/// `news_item_relevant`'s own doc comment for its remaining narrow cuts.
/// The live `NewsSelector`: real `--check-news` relevance over one
/// repo's `metadata/news`, evaluated with the same `news_item_valid` /
/// `news_item_relevant` used everywhere else. `run_check_news` reads each
/// repo's unread ids through one of these rather than inlining the scan,
/// so the director's news slot carries the production traffic; the state
/// files (`.read`/`.skip`/`.unread` under
/// `<root>/var/lib/gentoo/news`) are read here and written back by the
/// caller, mirroring real `NewsManager.updateItems` + `getUnreadItems`.
pub struct FilesystemNews<'a> {
    /// The repo checkout (`metadata/news` lives under it).
    pub repo_location: &'a Path,
    /// The `::reponame` owning the state files.
    pub repo_name: &'a str,
    /// The `${ROOT}` the `Display-If-Installed` atoms match against and
    /// the state files live under.
    pub root: &'a Path,
    /// The profile/`ARCH` the `Display-If-Profile` / `Display-If-Keyword`
    /// restrictions match against (backlog #258).
    pub context: &'a NewsContext,
}

/// What real `NewsItem.isRelevant` passes its non-vdb restrictions
/// (`news.py:406-423`) plus the ownership real
/// `NewsManager.__init__` gives the state files (`news.py:79-84`).
/// Built once per run by [`news_context`].
#[derive(Debug, Clone, Default)]
pub struct NewsContext {
    /// Real `NewsManager._profile_path`: `realpath(settings.profile_path)`
    /// with the main repo's `<location>/profiles/` prefix removed (the
    /// full real path when the profile lives elsewhere); `None` with no
    /// profile at all.
    pub profile: Option<String>,
    /// Real `settings.profile_path` is set (a `make.profile` directory
    /// exists): `count_unread_news`'s gate (`news.py:470-479`). It can
    /// be set while `profile` is `None` (no main repo).
    pub has_profile: bool,
    /// Real `config.get("ARCH", "")`, the `Display-If-Keyword` operand.
    pub arch: String,
    /// Real `int(config["PORTAGE_INST_UID"])` (default 0).
    pub inst_uid: u32,
    /// Real `portage.data.portage_gid`: the host's `PORTAGE_GRPNAME`
    /// group (default `portage`), 0 when that group does not exist
    /// (`data.py:173-178`). Read from `/etc/group` only, where real's
    /// `grp.getgrnam` also sees NSS sources (LDAP, sssd).
    pub portage_gid: u32,
}

/// The pure result of one [`FilesystemNews::evaluate`]: the state-file
/// sets as found (`*_orig`) plus the re-evaluated sets the caller writes
/// back when they changed.
pub struct NewsEvaluation {
    /// `.unread` as found on disk.
    pub unread_orig: HashSet<String>,
    /// `.unread` after evaluation (newly relevant added, `.read`
    /// applied).
    pub unread: HashSet<String>,
    /// `.skip` as found on disk.
    pub skip_orig: HashSet<String>,
    /// `.skip` after evaluation (newly relevant added, permanent).
    pub skip: HashSet<String>,
}

impl FilesystemNews<'_> {
    /// Evaluate one repo's news items: every item not already in `.read`
    /// or `.skip` is (re-)evaluated, and a valid, relevant one is added
    /// to *both* the `unread` and `skip` accumulators. `None` when the
    /// repo has no `metadata/news` directory at all (nothing pending,
    /// nothing to write back).
    pub fn evaluate(&self) -> Option<NewsEvaluation> {
        let news_dir = self.repo_location.join("metadata/news");
        let Ok(entries) = portage_util::read_dir_entries(&news_dir) else {
            return None;
        };
        // `.read` (eselect news read) + `.skip` (updateItems' permanent
        // per-item skip list) -- an id in either is not counted.
        let news_state_dir = self.root.join("var/lib/gentoo/news");
        let read_state_file = |suffix: &str| -> HashSet<String> {
            grab_news_state_file(
                &news_state_dir.join(format!("news-{}.{}", self.repo_name, suffix)),
            )
            .into_iter()
            .collect()
        };
        let read_only = read_state_file("read");
        let skip_orig = read_state_file("skip");
        let unread_orig = read_state_file("unread");
        let read: HashSet<String> = read_only
            .iter()
            .cloned()
            .chain(skip_orig.iter().cloned())
            .collect();

        let ids: Vec<String> = entries
            .into_iter()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .collect();

        // Real `NewsManager.updateItems` (`news.py:112-190`): every item
        // not already in `.skip` is (re-)evaluated; a valid, relevant one
        // is added to *both* the `unread` and `skip` accumulators
        // (permanently -- once relevant, always in `.skip`, so it is
        // never re-evaluated again even if it later stops matching a
        // live `Display-If-*` restriction).
        let mut unread = unread_orig.clone();
        let mut skip = skip_orig.clone();
        for id in ids {
            if read.contains(&id) {
                continue;
            }
            let item = news_dir.join(&id).join(format!("{id}.en.txt"));
            let Ok(text) = std::fs::read_to_string(&item) else {
                continue;
            };
            if !news_item_valid(&text) {
                continue;
            }
            if news_item_relevant(&text, self.root, self.context) {
                unread.insert(id.clone());
                skip.insert(id);
            }
        }
        // Real `getUnreadItems`'s count is simply `len(.unread)` -- once
        // an item is added, it stays counted on every future run,
        // regardless of whether it would still evaluate as "relevant"
        // (it's `.skip`-permanent, never re-checked). The only thing
        // real ever *removes* from `.unread` is `eselect news read`,
        // which portuale doesn't implement as its own action; `.read`
        // models that tool's effect instead, applied lazily here each
        // run (real would have rewritten `.unread` at the time the user
        // ran it) rather than requiring a separate action to exist.
        for id in &read_only {
            unread.remove(id);
        }
        Some(NewsEvaluation {
            unread_orig,
            unread,
            skip_orig,
            skip,
        })
    }
}

impl mrg_director::NewsSelector for FilesystemNews<'_> {
    fn unread_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .evaluate()
            .map(|eval| eval.unread.into_iter().collect())
            .unwrap_or_default();
        ids.sort();
        ids
    }

    fn repo_name(&self) -> String {
        self.repo_name.to_string()
    }
}

/// One repo's unread-news evaluation plus the `updateItems` state
/// write-back, shared by `--check-news` and the GLEP 42 count notice
/// (backlog #196): every repo maps to its `len(.unread)` count (real
/// `NewsManager.getUnreadItems`, `news.py:234`) -- 0 when the repo has
/// no `metadata/news` directory at all. The `.unread`/`.skip` files are
/// rewritten (sorted, one id per line) only when their sets actually
/// changed, exactly like `run_check_news` always did.
pub(crate) fn unread_news_counts(
    repos: &[portage_repo::RepoConfig],
    root: &Path,
    context: &NewsContext,
) -> Vec<(String, usize)> {
    // Real `count_unread_news` (`news.py:470-479`): with no profile
    // path at all it prints two errors and counts nothing, because
    // profile restrictions cannot be evaluated.
    if !context.has_profile {
        eprintln!("!!! News items will not be displayed due to an invalid profile setting.");
        eprintln!("!!! Use eselect profile to update your profile.");
        return Vec::new();
    }
    let mut per_repo: Vec<(String, usize)> = Vec::new();
    for repo in repos {
        // The unread computation runs through the director's news slot:
        // one filesystem selector per repo, evaluated here. Write-back
        // of the state files stays the CLI layer's job (the selector is
        // a pure relevance read, like every other director slot).
        let selector = FilesystemNews {
            repo_location: &repo.location,
            repo_name: &repo.name,
            root,
            context,
        };
        // Real `updateItems` prepares the state directory first, even
        // for a repo with no `metadata/news` (its `listdir` raises
        // after, and `getUnreadItems` turns that into 0). When the
        // preparation fails it returns before touching the news dir,
        // and `getUnreadItems` counts the `.unread` file as it stands
        // (`news.py:206-239`).
        let news_state_dir = root.join("var/lib/gentoo/news");
        if !ensure_news_state_dir(&news_state_dir, context) {
            let unread =
                grab_news_state_file(&news_state_dir.join(format!("news-{}.unread", repo.name)));
            per_repo.push((repo.name.clone(), unread.len()));
            continue;
        }
        let Some(eval) = selector.evaluate() else {
            per_repo.push((repo.name.clone(), 0));
            continue;
        };
        write_news_state_if_changed(WriteNewsStateIfChangedArgs {
            news_state_dir: &root.join("var/lib/gentoo/news"),
            repo_name: &repo.name,
            suffix: "unread",
            orig: &eval.unread_orig,
            updated: &eval.unread,
            context,
        });
        write_news_state_if_changed(WriteNewsStateIfChangedArgs {
            news_state_dir: &root.join("var/lib/gentoo/news"),
            repo_name: &repo.name,
            suffix: "skip",
            orig: &eval.skip_orig,
            updated: &eval.skip,
            context,
        });
        per_repo.push((repo.name.clone(), eval.unread.len()));
    }
    per_repo
}

/// Real `portage.news.display_news_notifications` (`news.py:509-528`):
/// a leading blank line, then one ` * IMPORTANT: N news items need
/// reading for repository '<repo>'.` line per repo with a nonzero
/// count, then ` * Use eselect news read to view new items.` plus a
/// trailing blank line. The all-zero gate lives INSIDE, like real
/// (`news.py:526`, `if news_reader_display`): an all-zero map prints
/// nothing at all, so a future third caller cannot emit a dangling
/// "Use eselect…" block (review Minor #4). Shared by `--check-news`
/// and the backlog #196 count notice so the two can never drift apart.
pub(crate) fn print_news_notifications(per_repo: &[(String, usize)], color: &Colorizer) -> bool {
    let mut first = true;
    for (repo, count) in per_repo {
        if *count > 0 {
            if first {
                println!();
                first = false;
            }
            println!(
                "{} {count} news items need reading for repository '{repo}'.",
                color.c("WARN", " * IMPORTANT:")
            );
        }
    }
    if !first {
        println!(
            "{} Use {} to view new items.\n",
            color.c("WARN", " *"),
            color.c("GOOD", "eselect news read")
        );
    }
    !first
}

/// Real `display_news_notification`'s own gate (`post_emerge.py:38`):
/// `news` is in the resolved `FEATURES`. Pure predicate so the gate
/// stays unit-pinned independently of the counting.
pub(crate) fn news_notice_enabled(config: &portage_profile::Config) -> bool {
    config_features_list(config).iter().any(|t| t == "news")
}

/// Backlog #196: real `_emerge/post_emerge.py::display_news_notification`
/// (`post_emerge.py:37-46`): print the GLEP 42 count notice when `news`
/// is in the resolved `FEATURES` and at least one repo has a nonzero
/// unread count (real `count_unread_news`, `news.py:447-506`, reusing the
/// existing `FilesystemNews` evaluation -- no second news parser). No
/// `--quiet` gate: real's own function consults neither `--quiet` nor
/// `--ask` (the m185 probe's `--quiet` shape shows the notice surfacing
/// mid-merge through stdio buffering, which is not ported).
/// Returns real `display_news_notification`'s own boolean (whether the
/// notice printed): the `--ask` + `--read-news` prompt below only fires
/// when the notice actually printed (real `actions.py:4268-4281`
/// short-circuits on its falsy return).
/// The all-zero gate lives in `print_news_notifications` (like real
/// `news.py:526`), so this calls it unconditionally past the FEATURES
/// gate.
pub(crate) fn display_news_notice_if_any(
    repos: &[portage_repo::RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    color: &Colorizer,
) -> bool {
    if !news_notice_enabled(config) {
        return false;
    }
    let context = news_context(repos, &config_root_from_env(), config);
    let per_repo = unread_news_counts(repos, root, &context);
    print_news_notifications(&per_repo, color)
}

/// Arguments of [`offer_news_reading`].
pub(crate) struct OfferNewsReadingArgs<'a> {
    pub(crate) ask: bool,
    pub(crate) read_news: bool,
    pub(crate) notice_printed: bool,
    pub(crate) color: &'a Colorizer,
}

/// Backlog #231 (c): real `_emerge/actions.py::run_action`
/// (`actions.py:4266-4281`) -- when the pre-resolution notice printed
/// *and* `--ask` *and* `--read-news` are given, real asks "Would you
/// like to read the news items while calculating dependencies?" (real
/// `UserQuery.query` with the `--ask-enter-invalid` flag) and spawns
/// `eselect news read` on "Yes" (stdio inherited, return code ignored).
/// A "No" answer skips the spawn and continues into the
/// resolve -- only the merge-list prompt quits the run. EOF or a read
/// error (backlog #234) prints real `UserQuery.query`'s own
/// `Interrupted.` (inside `ask_yes_no`, `_emerge/UserQuery.py:74-76`)
/// and reports `true` so the caller exits `128 + SIGINT` before
/// `action_build`, exactly like real `actions.py:4270-4281` (whose
/// `sys.exit` never reaches `action_build` either).
/// When `eselect`
/// is missing the spawn raises `OSError`, and real prints `Please
/// install eselect to use this feature.` to stderr. `--ask` without a
/// TTY never reaches here (the up-front `stdin_is_tty` gate exits
/// first), matching real `actions.py:3917-3926`; under `--pretend`
/// `ask` is already false, matching real's `--pretend` gate around the
/// whole block.
pub(crate) fn offer_news_reading(args: OfferNewsReadingArgs<'_>) -> bool {
    let OfferNewsReadingArgs {
        ask,
        read_news,
        notice_printed,
        color,
    } = args;
    if !(ask && read_news && notice_printed) {
        return false;
    }
    match ask_yes_no(
        color,
        "Would you like to read the news items while calculating dependencies?",
    ) {
        Some(true) => {
            match std::process::Command::new("eselect")
                .args(["news", "read"])
                .status()
            {
                Ok(_) => {}
                Err(_) => eprintln!("Please install eselect to use this feature."),
            }
            false
        }
        Some(false) => false,
        None => true,
    }
}

/// Backlog #196 fix round 1: real `_emerge/post_emerge.py:112-117` --
/// when the vdb did not change and the run was `--pretend`,
/// `post_emerge` still runs `display_news_notification` before
/// returning. Real reaches this arm for EVERY `action_build` retval:
/// `run_action` calls `post_emerge` unconditionally (the final
/// `retval = action_build(...); post_emerge(...)` in the build branch,
/// `actions.py:4289-4297`), so a failed `--pretend` resolve prints the
/// notice too -- every `--pretend` exit of the build path below ends
/// with this call. (Usage-error returns above the resolve never reach
/// `post_emerge` in real either, so they stay silent here as well.)
/// `--json` is a portuale-only format with no real counterpart and
/// stays machine-readable: no notice on that path.
pub(crate) fn pretend_end_news_notice(
    pretend: bool,
    repos: &[portage_repo::RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    color: &Colorizer,
) {
    if pretend {
        display_news_notice_if_any(repos, root, config, color);
    }
}

/// Backlog #196 fix round 1: real `post_emerge.py:155` prints the
/// notice regardless of retval (`retval` only feeds `exit_msg`,
/// `:104-108`) -- but only when the vdb actually changed (`:112-117`
/// early-returns otherwise). `changed` is that gate: the caller passes
/// `unmerged.len() < entries.len()` (at least one entry landed, read
/// off the vdb the way `entries_not_merged` already does for the
/// resume list). A total failure with nothing merged -- e.g. a digest
/// failure on the first package -- stays silent, exactly like real.
pub(crate) fn merge_failure_news_notice(
    changed: bool,
    repos: &[portage_repo::RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    color: &Colorizer,
) {
    if changed {
        display_news_notice_if_any(repos, root, config, color);
    }
}
/// Arguments of [`run_check_news`].
pub(crate) struct RunCheckNewsArgs<'a> {
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) quiet: bool,
    pub(crate) color: &'a Colorizer,
}

pub(crate) fn run_check_news(args: RunCheckNewsArgs<'_>) -> ExitCode {
    let RunCheckNewsArgs {
        repos,
        root,
        config_root,
        config,
        quiet,
        color,
    } = args;
    let context = news_context(repos, config_root, config);
    let per_repo = unread_news_counts(repos, root, &context);
    let any = per_repo.iter().any(|(_, count)| *count > 0);

    if any {
        // Real `display_news_notifications`.
        print_news_notifications(&per_repo, color);
    } else if !quiet {
        // Real `print("", colorize("GOOD", "*"), "No news items were found.")`.
        println!(" {} No news items were found.", color.c("GOOD", "*"));
    }
    ExitCode::SUCCESS
}

/// Real `grabfile` (`util/__init__.py`) over one news state file: each
/// line split on whitespace, cut at the first `#`-led token, re-joined;
/// empty results dropped. A missing file reads as empty.
pub(crate) fn grab_news_state_file(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| {
            line.split_whitespace()
                .take_while(|tok| !tok.starts_with('#'))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// Arguments of [`write_news_state_if_changed`].
pub(crate) struct WriteNewsStateIfChangedArgs<'a> {
    pub(crate) news_state_dir: &'a Path,
    pub(crate) repo_name: &'a str,
    pub(crate) suffix: &'a str,
    pub(crate) orig: &'a std::collections::HashSet<String>,
    pub(crate) updated: &'a std::collections::HashSet<String>,
    pub(crate) context: &'a NewsContext,
}

/// Real `NewsManager.updateItems`'s own `write_atomic` calls
/// (`news.py:183-200`): `news-<repo>.<suffix>` gets rewritten -- one id
/// per line, sorted -- only when `updated` differs from `orig`, i.e. the
/// set genuinely changed this run, and then gets real's
/// `apply_secpass_permissions(uid=PORTAGE_INST_UID, gid=portage_gid,
/// mode=0o064, mask=0)`: group `portage`, group read/write and world
/// read on top of the umask mode (a root run lands `0664 root:portage`,
/// backlog #258). The state directory itself was already prepared by
/// [`ensure_news_state_dir`].
pub(crate) fn write_news_state_if_changed(args: WriteNewsStateIfChangedArgs<'_>) {
    let WriteNewsStateIfChangedArgs {
        news_state_dir,
        repo_name,
        suffix,
        orig,
        updated,
        context,
    } = args;
    if updated == orig {
        return;
    }
    let mut ids: Vec<&String> = updated.iter().collect();
    ids.sort();
    let mut out = String::new();
    for id in ids {
        out.push_str(id);
        out.push('\n');
    }
    let path = news_state_dir.join(format!("news-{repo_name}.{suffix}"));
    let tmp = news_state_dir.join(format!(".news-{repo_name}.{suffix}.tmp"));
    if std::fs::write(&tmp, &out).is_ok() && std::fs::rename(&tmp, &path).is_ok() {
        apply_news_permissions(&path, context, 0o064);
    }
}

/// Real `updateItems`'s preamble (`news.py:121-133`): `ensure_dirs(
/// unread_path, uid=PORTAGE_INST_UID, gid=portage_gid, mode=0o074,
/// mask=0)` -- a permission failure returns before anything is read or
/// written -- then `os.access(unread_path, os.W_OK)`, same early return.
/// `false` means "no write-back this run": the caller then counts the
/// `.unread` file as found, exactly like real `getUnreadItems` reading
/// `len(grabfile(unread))` after the early return.
pub(crate) fn ensure_news_state_dir(dir: &Path, context: &NewsContext) -> bool {
    if std::fs::create_dir_all(dir).is_err() || !apply_news_permissions(dir, context, 0o074) {
        return false;
    }
    let Ok(c_dir) = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: `c_dir` is a valid NUL-terminated path for the call.
    unsafe { libc::access(c_dir.as_ptr(), libc::W_OK) == 0 }
}

/// Real `apply_permissions` with `mask=0` (`util/__init__.py:1145`):
/// chown to `PORTAGE_INST_UID:portage_gid` when either differs, then
/// OR the `mode` bits into the existing mode when any is missing.
/// Ownership is only changed by a root caller: a non-root caller is
/// real's unprivileged mode (`PORTAGE_INST_UID` and the portage group
/// become the caller's own, so the files it creates already match) or,
/// for the state files, real's `apply_secpass_permissions` skipping
/// what `secpass < 2` cannot do. One narrow difference remains for the
/// directory: real's `ensure_dirs` has no secpass gate, so a non-root
/// caller facing a writable directory it does not own fails the chown
/// and skips the update, where this proceeds. `false` on a failed
/// chown/chmod (real `OperationNotPermitted`).
pub(crate) fn apply_news_permissions(path: &Path, context: &NewsContext, mode: u32) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    // SAFETY: `geteuid` has no preconditions.
    let is_root = unsafe { libc::geteuid() } == 0;
    if is_root
        && (meta.uid() != context.inst_uid || meta.gid() != context.portage_gid)
        && std::os::unix::fs::chown(path, Some(context.inst_uid), Some(context.portage_gid))
            .is_err()
    {
        return false;
    }
    let st_mode = meta.mode() & 0o7777;
    if st_mode & mode != mode
        && std::fs::set_permissions(path, std::fs::Permissions::from_mode(st_mode | mode)).is_err()
    {
        return false;
    }
    true
}

/// Build the [`NewsContext`] real `NewsManager` works from: the profile
/// path (`news.py:88-98`), `ARCH`, `PORTAGE_INST_UID` and the portage
/// group id.
pub(crate) fn news_context(
    repos: &[portage_repo::RepoConfig],
    config_root: &Path,
    config: &portage_profile::Config,
) -> NewsContext {
    let var = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| config.other_vars.get(name).cloned())
    };
    let grpname = var("PORTAGE_GRPNAME").unwrap_or_else(|| "portage".to_string());
    NewsContext {
        profile: news_profile_path(repos, config_root),
        has_profile: config_profile_dir(config_root).is_some(),
        arch: config.other_vars.get("ARCH").cloned().unwrap_or_default(),
        inst_uid: var("PORTAGE_INST_UID")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0),
        portage_gid: host_group_gid(&grpname).unwrap_or(0),
    }
}

/// Real `NewsManager.__init__`'s `_profile_path` (`news.py:88-98`):
/// `realpath` of `settings.profile_path` -- `<config_root>/etc/portage/
/// make.profile`, else the deprecated `<config_root>/etc/make.profile`
/// (`LocationsManager.py:119-143`), else none -- with the main repo's
/// `<location>/profiles/` prefix removed. Real builds that prefix from
/// the configured location without resolving it, so a profile outside
/// it stays a full real path (and matches no `Display-If-Profile`).
pub(crate) fn news_profile_path(
    repos: &[portage_repo::RepoConfig],
    config_root: &Path,
) -> Option<String> {
    let main = repos.iter().find(|r| r.is_main)?;
    let make_profile = config_profile_dir(config_root)?;
    let real = std::fs::canonicalize(&make_profile).ok()?;
    let real = real.to_string_lossy().into_owned();
    let base = format!("{}/", main.location.join("profiles").to_string_lossy());
    Some(real.strip_prefix(&base).map(str::to_string).unwrap_or(real))
}

/// Real `settings.profile_path` (`LocationsManager.py:119-143`):
/// `<config_root>/etc/portage/make.profile`, else the deprecated
/// `<config_root>/etc/make.profile`, whichever is a directory.
pub(crate) fn config_profile_dir(config_root: &Path) -> Option<std::path::PathBuf> {
    [
        config_root.join("etc/portage/make.profile"),
        config_root.join("etc/make.profile"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

/// Real `grp.getgrnam(name).gr_gid` for `portage.data.portage_gid`: the
/// running host's group database (`/etc/group`), never `${ROOT}`'s.
pub(crate) fn host_group_gid(name: &str) -> Option<u32> {
    std::fs::read_to_string("/etc/group")
        .ok()?
        .lines()
        .find_map(|line| {
            let mut f = line.split(':');
            (f.next() == Some(name))
                .then(|| f.nth(1).and_then(|g| g.parse::<u32>().ok()))
                .flatten()
        })
}

/// Real `NewsItem.isValid`: a `News-Item-Format:` header whose value
/// `fnmatch`es `[12].*` (i.e. starts `1.` or `2.`), and each
/// `Display-If-Installed` atom passes the format-bound `isvalidatom`
/// gate. Returns the format version as `Some(true)` for a 1.x item and
/// `Some(false)` for 2.x -- real `NewsItem.parse`'s own
/// `fnmatch.fnmatch(news_format, "[12].*")` break-on-first-match is
/// mirrored (the first `1.*`/`2.*` header line wins). `None` carries
/// "no valid format header" (real `_formatRE` miss -> invalid).
pub(crate) fn news_item_format(text: &str) -> Option<bool> {
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("News-Item-Format:").map(str::trim) {
            if v.starts_with("1.") || v == "1" {
                return Some(true);
            }
            if v.starts_with("2.") || v == "2" {
                return Some(false);
            }
        }
    }
    None
}

pub(crate) fn news_item_valid(text: &str) -> bool {
    let Some(is_1x) = news_item_format(text) else {
        return false;
    };
    // Real `NewsItem.isValid` also fails the whole item when any
    // `Display-If-Installed:` restriction's own atom is malformed
    // (`DisplayInstalledRestriction.isValid` -> `isvalidatom(atom,
    // eapi=…)` -> `InvalidAtom` -> `self._valid = False`).
    //
    // The `eapi="0"` (1.x) vs `eapi="5"` (2.x) split is *not* a
    // `portage_dep` EAPI parametrization -- it is a narrow field gate,
    // "does it parse at all" untouched: real EAPI 0 lacks slot deps
    // (`slot_deps` is EAPI >= 1) and USE deps (`use_deps` is EAPI >= 2),
    // so a 1.x Display-If-Installed atom is legal only when it carries
    // no `:slot`/`:slot/sub` (also no `:=`/`:*` slot operator) and no
    // `[use]`. A 2.x atom keeps the permissive parse-at-all check (its
    // own grammar is exactly the EAPI-5+ superset `parse_atom` already
    // implements). No crate EAPI parameterization is added -- Part 3's
    // non-goal is about ebuild-conditional EAPI logic, not this
    // one-off legacy news-format rule.
    //
    // Real `DisplayProfileRestriction.isValid` (`news.py:398-404`) fails
    // a 1.x item whose `Display-If-Profile:` value holds a `*` anywhere
    // but a trailing `/*` (`_valid_profile_RE`, `^[^*]+(/\*)?$`); a 2.x
    // value is always valid.
    let profile_valid = |value: &str| {
        !is_1x || !value.contains('*') || {
            let head = value.strip_suffix("/*").unwrap_or(value);
            !head.is_empty() && !head.contains('*')
        }
    };
    text.lines()
        .filter_map(|l| l.strip_prefix("Display-If-Profile:").map(str::trim))
        .all(profile_valid)
        && text
            .lines()
            .filter_map(|l| l.strip_prefix("Display-If-Installed:").map(str::trim))
            .all(|atom| match parse_atom(atom) {
                Some(a) => {
                    !is_1x
                        || (a.slot.is_none()
                            && a.sub_slot.is_none()
                            && a.slot_operator.is_none()
                            && a.use_deps.is_none())
                }
                None => false,
            })
}

/// Real `NewsItem.isRelevant` (`news.py:269-306`): no restriction ->
/// relevant; otherwise each `Display-If-*` kind is OR'd within and
/// AND'd across. The three kinds (backlog #258 added the last two):
///
/// - `Display-If-Installed` -- real `DisplayInstalledRestriction.
///   checkRestriction` is `vardb.match(self.atom)`: a full atom match,
///   version operators (`>=cat/pkg-1`), slot/sub-slot (`cat/pkg:2/3`)
///   and all, against every installed version of the atom's `cat/pkg`.
///   A `[use]`-dep is post-filtered against the matched version's own
///   vdb `IUSE`/`USE` (`use_deps_satisfied`), since portuale's
///   `match_from_list` ignores use-deps. A malformed atom makes the
///   whole item *invalid* (see `news_item_valid`), not merely
///   unsatisfied.
/// - `Display-If-Profile` -- real `DisplayProfileRestriction.
///   checkRestriction` (`news.py:406-409`): a 2.x item's `…/*` value
///   matches every profile under that prefix (`startswith` of the value
///   minus its `*`), anything else must equal the profile path exactly.
///   No profile at all matches nothing.
/// - `Display-If-Keyword` -- real `DisplayKeywordRestriction.
///   checkRestriction` (`news.py:422-423`): `config.get("ARCH", "")`
///   equals the value, verbatim (so `~amd64` never matches).
///
/// Real only matches a restriction line that ends in a newline
/// (`_installedRE` and friends end in `\n`); a header on a file's final,
/// unterminated line is the one shape not mirrored here.
pub(crate) fn news_item_relevant(text: &str, root: &Path, context: &NewsContext) -> bool {
    let is_2x = news_item_format(text) == Some(false);
    let values = |key: &str| -> Vec<&str> {
        text.lines()
            .filter_map(|l| l.strip_prefix(key).map(str::trim))
            .collect()
    };
    let installed_atoms = values("Display-If-Installed:");
    let profiles = values("Display-If-Profile:");
    let keywords = values("Display-If-Keyword:");
    let installed_ok = installed_atoms.is_empty()
        || installed_atoms.iter().any(|atom_str| {
            let Some(atom) = parse_atom(atom_str) else {
                return false;
            };
            let use_deps = atom.use_deps.as_deref().filter(|d| !d.is_empty());
            portage_repo::installed_candidates(root, &atom.category, &atom.package)
                .iter()
                .any(|(version, slot, sub_slot)| {
                    let candidate = format!(
                        "{}/{}-{version}:{slot}/{sub_slot}",
                        atom.category, atom.package
                    );
                    if !match_from_list(atom_str, &[candidate.as_str()])
                        .is_some_and(|m| !m.is_empty())
                    {
                        return false;
                    }
                    // Real `vardb.match` also enforces the atom's use-deps
                    // against this version's own recorded USE.
                    match use_deps {
                        None => true,
                        Some(uds) => {
                            let (iuse, use_flags) = portage_repo::installed_pkg_iuse_and_use(
                                root,
                                &atom.category,
                                &atom.package,
                                version,
                            );
                            portage_dep::use_deps_satisfied(uds, &iuse, &use_flags)
                        }
                    }
                })
        });
    let profile_ok = profiles.is_empty()
        || profiles
            .iter()
            .any(|value| match context.profile.as_deref() {
                Some(profile) => {
                    if is_2x && value.ends_with("/*") {
                        profile.starts_with(&value[..value.len() - 1])
                    } else {
                        profile == *value
                    }
                }
                None => false,
            });
    let keyword_ok = keywords.is_empty() || keywords.contains(&context.arch.as_str());
    installed_ok && profile_ok && keyword_ok
}
