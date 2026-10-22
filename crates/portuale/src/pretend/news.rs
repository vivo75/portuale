//! GLEP 42 news.

use crate::pretend::*;

/// Print the `IMPORTANT: N news items need reading` block if any repo has
/// unread items, else ` * No news items were found.` (unless `--quiet`).
/// A news item is valid iff its `News-Item-Format:` header matches `[12].*`,
/// and relevant iff every `Display-If-*` kind it carries has a matching
/// value. The printed count is `len(.unread)`, not a live recompute: once
/// counted, an item stays counted until removed from `.unread`.
/// Portage: _emerge/actions.py; portage/news.py, NewsManager
pub struct FilesystemNews<'a> {
    /// The repo checkout (`metadata/news` lives under it).
    pub repo_location: &'a Path,
    /// The `::reponame` owning the state files.
    pub repo_name: &'a str,
    /// The `${ROOT}` the `Display-If-Installed` atoms match against and
    /// the state files live under.
    pub root: &'a Path,
    /// The profile/`ARCH` the `Display-If-Profile` / `Display-If-Keyword`
    /// restrictions match against.
    pub context: &'a NewsContext,
}

/// The non-vdb restrictions plus the state-file ownership, built once per
/// run by [`news_context`].
/// Portage: portage/news.py, NewsManager
#[derive(Debug, Clone, Default)]
pub struct NewsContext {
    /// The profile path with the main repo's `<location>/profiles/` prefix
    /// removed (the full path when the profile lives elsewhere); `None`
    /// with no profile at all.
    pub profile: Option<String>,
    /// Whether a `make.profile` directory exists. It can be set while
    /// `profile` is `None` (no main repo).
    pub has_profile: bool,
    /// The `Display-If-Keyword` operand.
    pub arch: String,
    /// `PORTAGE_INST_UID` (default 0).
    pub inst_uid: u32,
    /// The host's `PORTAGE_GRPNAME` group (default `portage`), 0 when that
    /// group does not exist. Read from `/etc/group` only.
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
    /// to both the `unread` and `skip` accumulators. `None` when the repo
    /// has no `metadata/news` directory at all.
    /// Portage: portage/news.py, NewsManager.updateItems
    pub fn evaluate(&self) -> Option<NewsEvaluation> {
        let news_dir = self.repo_location.join("metadata/news");
        let Ok(entries) = portage_util::read_dir_entries(&news_dir) else {
            return None;
        };
        // `.read` + `.skip`: an id in either is not counted.
        // Portage: portage/news.py, NewsManager.updateItems
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

        // Every item not already in `.skip` is (re-)evaluated; a valid,
        // relevant one joins both accumulators, permanently.
        // Portage: portage/news.py, NewsManager.updateItems
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
        // The count is `len(.unread)`: once added, an item stays counted.
        // `.read` models `eselect news read`, applied lazily each run.
        // Portage: portage/news.py, getUnreadItems
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

/// One repo's unread-news evaluation plus the state write-back, shared by
/// `--check-news` and the GLEP 42 count notice: every repo maps to its
/// `len(.unread)` count (0 with no `metadata/news` directory). The
/// `.unread`/`.skip` files are rewritten only when their sets changed.
/// Portage: portage/news.py, NewsManager
pub(crate) fn unread_news_counts(
    repos: &[portage_repo::RepoConfig],
    root: &Path,
    context: &NewsContext,
) -> Vec<(String, usize)> {
    // With no profile path at all, print two errors and count nothing:
    // profile restrictions cannot be evaluated.
    // Portage: portage/news.py, count_unread_news
    if !context.has_profile {
        eprintln!("!!! News items will not be displayed due to an invalid profile setting.");
        eprintln!("!!! Use eselect profile to update your profile.");
        return Vec::new();
    }
    let mut per_repo: Vec<(String, usize)> = Vec::new();
    for repo in repos {
        // The unread computation runs through the director's news slot;
        // write-back of the state files stays the CLI layer's job.
        let selector = FilesystemNews {
            repo_location: &repo.location,
            repo_name: &repo.name,
            root,
            context,
        };
        // The state directory is prepared first, even for a repo with no
        // `metadata/news`; when the preparation fails, count the `.unread`
        // file as it stands.
        // Portage: portage/news.py, updateItems; getUnreadItems
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

/// One ` * IMPORTANT: N news items need reading for repository '<repo>'.`
/// line per repo with a nonzero count, then the `eselect news read` hint.
/// An all-zero map prints nothing at all. Shared by `--check-news` and the
/// count notice so the two can never drift apart.
/// Portage: portage/news.py, display_news_notifications
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

/// Whether `news` is in the resolved `FEATURES`: the notice's own gate.
/// Pure predicate so the gate stays unit-pinned.
/// Portage: post_emerge.py, display_news_notification
pub(crate) fn news_notice_enabled(config: &portage_profile::Config) -> bool {
    config_features_list(config).iter().any(|t| t == "news")
}

/// Print the GLEP 42 count notice when `news` is in the resolved `FEATURES`
/// and at least one repo has a nonzero unread count. No `--quiet` gate.
/// Returns whether the notice printed: the `--ask` + `--read-news` prompt
/// only fires when it did.
/// Portage: post_emerge.py, display_news_notification
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

/// When the pre-resolution notice printed and `--ask` + `--read-news` are
/// given, ask "Would you like to read the news items while calculating
/// dependencies?" and spawn `eselect news read` on "Yes" (return code
/// ignored). A "No" answer skips the spawn and continues into the resolve.
/// Missing `eselect` prints `Please install eselect to use this feature.`
/// Portage: _emerge/actions.py, run_action
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

/// While the `--pretend` plan cache captures the resolve+display bytes,
/// the GLEP 42 count notice stays live (it depends on the news read-marker
/// files, which are not cache inputs): the capturer sets this, and
/// [`pretend_end_news_notice`] becomes a no-op until it is cleared. The
/// caller then prints the notice itself, after restoring the streams.
/// Process-wide like the other pretend globals; only ever touched around
/// one synchronous capture on this thread.
pub(crate) static PLAN_CACHE_CAPTURING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// On a `--pretend` run, still print the count notice at the end, even when
/// resolution failed. `--json` stays machine-readable: no notice there.
/// Portage: post_emerge.py, post_emerge
pub(crate) fn pretend_end_news_notice(
    pretend: bool,
    repos: &[portage_repo::RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    color: &Colorizer,
) {
    if PLAN_CACHE_CAPTURING.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if pretend {
        display_news_notice_if_any(repos, root, config, color);
    }
}

/// On a merge failure, print the notice only when the vdb actually changed
/// (`changed`: at least one entry landed). A total failure with nothing
/// merged stays silent.
/// Portage: post_emerge.py, post_emerge
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

/// Read one news state file: each line split on whitespace, cut at the
/// first `#`-led token, re-joined; empty results dropped. A missing file
/// reads as empty.
/// Portage: portage/util/__init__.py, grabfile
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

/// Rewrite `news-<repo>.<suffix>` (one id per line, sorted) only when the
/// set genuinely changed this run, then apply the secpass permissions.
/// The state directory itself was already prepared by [`ensure_news_state_dir`].
/// Portage: portage/news.py, NewsManager.updateItems
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

/// Prepare the state directory first; a permission failure returns before
/// anything is read or written. `false` means "no write-back this run":
/// the caller then counts the `.unread` file as found.
/// Portage: portage/news.py, NewsManager.updateItems
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

/// Chown to `PORTAGE_INST_UID:portage_gid` when either differs (root caller
/// only), then OR the `mode` bits into the existing mode when any is
/// missing. `false` on a failed chown/chmod.
/// Portage: portage/util/__init__.py, apply_permissions
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

/// Build the [`NewsContext`]: the profile path, `ARCH`, `PORTAGE_INST_UID`
/// and the portage group id.
/// Portage: portage/news.py, NewsManager
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

/// The profile path with the main repo's `<location>/profiles/` prefix
/// removed; a profile outside it stays a full path (matching no
/// `Display-If-Profile`).
/// Portage: portage/news.py, NewsManager
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

/// The profile directory: `<config_root>/etc/portage/make.profile`, else
/// the deprecated `<config_root>/etc/make.profile`, whichever is a directory.
/// Portage: portage/locations.py, LocationsManager
pub(crate) fn config_profile_dir(config_root: &Path) -> Option<std::path::PathBuf> {
    [
        config_root.join("etc/portage/make.profile"),
        config_root.join("etc/make.profile"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

/// The running host's group database (`/etc/group`), never `${ROOT}`'s.
/// Portage: portage/data.py, portage_gid
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

/// The format version as `Some(true)` for a 1.x item and `Some(false)` for
/// 2.x; `None` means "no valid format header". The first `1.*`/`2.*` header
/// line wins.
/// Portage: portage/news.py, NewsItem.parse
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
    // A malformed `Display-If-Installed` atom fails the whole item, and a
    // 1.x item only allows atoms without slot info or use-deps (EAPI 0
    // lacks both); a 2.x atom keeps the permissive parse-at-all check. A
    // 1.x `Display-If-Profile` value allows `*` only as a trailing `/*`.
    // Portage: portage/news.py, NewsItem.isValid
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

/// No restriction -> relevant; otherwise each `Display-If-*` kind is OR'd
/// within and AND'd across:
/// - `Display-If-Installed`: a full atom match against every installed
///   version of the atom's `cat/pkg`, with `[use]`-deps post-filtered
///   against the matched version's own vdb `IUSE`/`USE`.
/// - `Display-If-Profile`: a 2.x `…/*` value matches every profile under
///   that prefix, anything else must equal the profile path exactly.
/// - `Display-If-Keyword`: verbatim equality with `ARCH`.
///
/// Portage: portage/news.py, NewsItem.isRelevant
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
                    // The atom's use-deps are enforced against this version's
                    // own recorded USE.
                    // Portage: portage/news.py, DisplayInstalledRestriction
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
