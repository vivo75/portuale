// Portage SRC_URI flattening, Manifest digest parsing, and content-digest
// verification -- the non-network half of fetching a package's sources
// (`portuale/src/fetch.rs` holds the network half, the `wget` subprocess
// invocation this crate deliberately has nothing to do with, so its logic
// stays 100% testable offline).
//
// SRC_URI grammar (PMS 3.1.6, plus Portage's `mirror://`, `mirror+` and
// `fetch+` extensions): whitespace-separated URIs with optional `-> name`
// renames, under recursively nested `flag? ( ... )` / `!flag? ( ... )`
// groups. No `||` groups (PMS 8.2.6.5). See `SrcUriEntry` and `layout`
// for how a mirror root becomes a file URL.
//
// Manifest2 `DIST` lines (`lib/portage/manifest.py`), verified via the
// standard BLAKE2b-512 and SHA-512 algorithms. Known v1 scope cuts (see
// notes): deterministic (unshuffled) `mirror://` candidates, no
// `custommirrors["local"]` fast path, only `BLAKE2B`/`SHA512` verified,
// `DIST` lines only.

pub mod layout;
pub mod mirror_cache;

use std::collections::HashMap;
use std::path::Path;

/// One Portage `Manifest` `DIST` line's digest record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistfileDigests {
    pub size: u64,
    /// Hash algorithm name (`"BLAKE2B"`/`"SHA512"`, Portage Manifest2
    /// casing) -> lowercase hex digest, as the `Manifest` records it.
    pub hashes: HashMap<String, String>,
}

/// Error from Manifest/SRC_URI/digest handling. The variants mirror
/// Portage `lib/portage/manifest.py` / `fetch.py` message shapes so
/// `Display` reproduces them byte-for-byte (the contract suite pins them).
#[derive(Debug)]
pub enum Error {
    /// `{path}: {source}` -- an `io::Error` while reading a real file.
    Io {
        path: String,
        source: std::io::Error,
    },
    /// `SRC_URI: expected "(" after {tok:?}`
    SrcUriExpectedOpenAfter { tok: String },
    /// `SRC_URI: unterminated {tok:?} group`
    SrcUriUnterminated { tok: String },
    /// `SRC_URI: unexpected {tok:?}`
    SrcUriUnexpected { tok: String },
    /// `SRC_URI: missing filename after "->"`
    SrcUriMissingFilename,
    /// `SRC_URI: unexpected token {tok:?}`
    SrcUriUnexpectedToken { tok: String },
    /// `{path}: size mismatch (expected {expected}, got {got})`
    SizeMismatch {
        path: String,
        expected: u64,
        got: usize,
    },
    /// `{path}: {algo} mismatch (expected {expected}, got {actual})`
    HashMismatch {
        path: String,
        algo: String,
        expected: String,
        actual: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "{path}: {source}"),
            Error::SrcUriExpectedOpenAfter { tok } => {
                write!(f, "SRC_URI: expected \"(\" after {tok:?}")
            }
            Error::SrcUriUnterminated { tok } => {
                write!(f, "SRC_URI: unterminated {tok:?} group")
            }
            Error::SrcUriUnexpected { tok } => write!(f, "SRC_URI: unexpected {tok:?}"),
            Error::SrcUriMissingFilename => {
                write!(f, "SRC_URI: missing filename after \"->\"")
            }
            Error::SrcUriUnexpectedToken { tok } => {
                write!(f, "SRC_URI: unexpected token {tok:?}")
            }
            Error::SizeMismatch {
                path,
                expected,
                got,
            } => write!(f, "{path}: size mismatch (expected {expected}, got {got})"),
            Error::HashMismatch {
                path,
                algo,
                expected,
                actual,
            } => write!(
                f,
                "{path}: {algo} mismatch (expected {expected}, got {actual})"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

/// Portage `Manifest.parseManifest2`, narrowed to `DIST` lines only. A
/// missing `Manifest` file is an empty map, not an error.
pub fn parse_manifest(manifest_path: &Path) -> Result<HashMap<String, DistfileDigests>, Error> {
    let text = match std::fs::read_to_string(manifest_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => {
            return Err(Error::Io {
                path: manifest_path.display().to_string(),
                source: e,
            });
        }
    };

    let mut out = HashMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("DIST") {
            continue;
        }
        let Some(name) = parts.next() else { continue };
        let Some(size) = parts.next().and_then(|s| s.parse::<u64>().ok()) else {
            continue;
        };
        let rest: Vec<&str> = parts.collect();
        let mut hashes = HashMap::new();
        let mut i = 0;
        while i + 1 < rest.len() {
            hashes.insert(rest[i].to_string(), rest[i + 1].to_string());
            i += 2;
        }
        out.insert(name.to_string(), DistfileDigests { size, hashes });
    }
    Ok(out)
}

/// One flattened `SRC_URI` entry: the remote URI, and the local filename
/// it should be saved as (the `->` rename, or the URI's own basename).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrcUriEntry {
    pub uri: String,
    pub filename: String,
    /// Portage `override_mirror` (`fetch.py`): this URI token had a
    /// `mirror+` prefix, re-permitting the public mirror list even under
    /// `RESTRICT=mirror`. Implies `override_fetch` too.
    pub override_mirror: bool,
    /// Portage `override_fetch` (`fetch.py`): `mirror+` or `fetch+`
    /// prefix, exempting this one URI under `RESTRICT=fetch`. (See notes.)
    pub override_fetch: bool,
}

fn basename(uri: &str) -> String {
    uri.rsplit('/').next().unwrap_or(uri).to_string()
}

/// Recursive-descent parser for the module grammar. `active(negated, flag)`
/// decides whether a `flag?`/`!flag?` group's contents are collected (see notes).
fn parse_list(
    tokens: &[&str],
    pos: &mut usize,
    active: &impl Fn(bool, &str) -> bool,
) -> Result<Vec<SrcUriEntry>, Error> {
    let mut out = Vec::new();
    while *pos < tokens.len() && tokens[*pos] != ")" {
        let tok = tokens[*pos];
        if let Some(flag) = tok.strip_suffix('?') {
            *pos += 1;
            if tokens.get(*pos) != Some(&"(") {
                return Err(Error::SrcUriExpectedOpenAfter {
                    tok: tok.to_string(),
                });
            }
            *pos += 1;
            let (negated, flag) = match flag.strip_prefix('!') {
                Some(f) => (true, f),
                None => (false, flag),
            };
            let inner = parse_list(tokens, pos, active)?;
            if tokens.get(*pos) != Some(&")") {
                return Err(Error::SrcUriUnterminated {
                    tok: tok.to_string(),
                });
            }
            *pos += 1;
            if active(negated, flag) {
                out.extend(inner);
            }
        } else if tok == "(" || tok == ")" {
            return Err(Error::SrcUriUnexpected {
                tok: tok.to_string(),
            });
        } else {
            *pos += 1;
            // Portage `fetch.py`: strip a `mirror+`/`fetch+`
            // prefix and record the override (`mirror+` implies `fetch+`).
            let (uri, override_mirror, override_fetch) =
                if let Some(rest) = tok.strip_prefix("mirror+") {
                    (rest, true, true)
                } else if let Some(rest) = tok.strip_prefix("fetch+") {
                    (rest, false, true)
                } else {
                    (tok, false, false)
                };
            let filename = if tokens.get(*pos) == Some(&"->") {
                *pos += 1;
                let Some(name) = tokens.get(*pos) else {
                    return Err(Error::SrcUriMissingFilename);
                };
                *pos += 1;
                name.to_string()
            } else {
                basename(uri)
            };
            out.push(SrcUriEntry {
                uri: uri.to_string(),
                filename,
                override_mirror,
                override_fetch,
            });
        }
    }
    Ok(out)
}

/// Flattens a Portage `SRC_URI` string into its ordered `(uri, filename)`
/// pairs; see the module docs for the grammar and `active`'s meaning.
pub fn flatten_src_uri(
    src_uri: &str,
    active: impl Fn(bool, &str) -> bool,
) -> Result<Vec<SrcUriEntry>, Error> {
    let tokens: Vec<&str> = src_uri.split_whitespace().collect();
    let mut pos = 0;
    let entries = parse_list(&tokens, &mut pos, &active)?;
    if pos != tokens.len() {
        return Err(Error::SrcUriUnexpectedToken {
            tok: tokens.get(pos).copied().unwrap_or("").to_string(),
        });
    }
    Ok(entries)
}

/// Portage `grabdict()` (`lib/portage/util/__init__.py`), narrowed to
/// `profiles/thirdpartymirrors`: one `<name> <url1> [<url2> ...]` entry
/// per line. `#` starts a comment; lines left with fewer than 2 tokens
/// are skipped; a missing file is an empty map (see notes).
pub fn parse_thirdpartymirrors(path: &Path) -> Result<HashMap<String, Vec<String>>, Error> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => {
            return Err(Error::Io {
                path: path.display().to_string(),
                source: e,
            });
        }
    };

    let mut out = HashMap::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line
            .split_whitespace()
            .take_while(|t| !t.starts_with('#'))
            .collect();
        if tokens.len() < 2 {
            continue;
        }
        out.insert(
            tokens[0].to_string(),
            tokens[1..].iter().map(|s| (*s).to_string()).collect(),
        );
    }
    Ok(out)
}

/// Portage `mirror://<name>/<path>` resolution (Portage `fetch.py`):
/// `<name>` is looked up in `custommirrors` first, then
/// `thirdpartymirrors`, expanding to `<mirror_root>/<path>` per root.
/// Unknown names and malformed tokens yield no candidates (Portage warns;
/// the caller still fails loudly when no candidate works). A
/// non-`mirror://` URI is returned unchanged (see notes).
pub fn resolve_mirror_candidates(
    uri: &str,
    custommirrors: &HashMap<String, Vec<String>>,
    thirdpartymirrors: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let Some(rest) = uri.strip_prefix("mirror://") else {
        return vec![uri.to_string()];
    };
    let Some(slash) = rest.find('/') else {
        return Vec::new();
    };
    let name = &rest[..slash];
    let path = &rest[slash + 1..];
    let expand = |roots: &HashMap<String, Vec<String>>| -> Vec<String> {
        roots
            .get(name)
            .map(|roots| {
                roots
                    .iter()
                    .map(|root| format!("{}/{}", root.trim_end_matches('/'), path))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut candidates = expand(custommirrors);
    candidates.extend(expand(thirdpartymirrors));
    candidates
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Portage `make.globals`'s default `FETCHCOMMAND` template, verbatim.
pub const DEFAULT_FETCHCOMMAND: &str = concat!(
    "wget -t 3 -T 60 --passive-ftp -U \"Portage (Gentoo, https://www.gentoo.org) ",
    "distfile-fetch\" -O \"${DISTDIR}/${FILE}\" \"${URI}\""
);

/// Portage `make.globals`'s `RESUMECOMMAND`: `FETCHCOMMAND` plus `-c`.
pub const DEFAULT_RESUMECOMMAND: &str = concat!(
    "wget -c -t 3 -T 60 --passive-ftp -U \"Portage (Gentoo, https://www.gentoo.org) ",
    "distfile-fetch\" -O \"${DISTDIR}/${FILE}\" \"${URI}\""
);

/// Portage `fetch.py`'s fetch-command family, resolved from the
/// config. `None` (and an absent protocol key) means the `make.globals` default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchCommands {
    pub fetchcommand: Option<String>,
    pub resumecommand: Option<String>,
    /// Keys are the upper-cased URI scheme (`HTTP`, `HTTPS`, ...).
    pub fetchcommand_proto: std::collections::HashMap<String, String>,
    pub resumecommand_proto: std::collections::HashMap<String, String>,
}

impl Default for FetchCommands {
    /// The `make.globals` defaults, no overrides.
    fn default() -> Self {
        Self {
            fetchcommand: Some(DEFAULT_FETCHCOMMAND.to_string()),
            resumecommand: Some(DEFAULT_RESUMECOMMAND.to_string()),
            fetchcommand_proto: std::collections::HashMap::new(),
            resumecommand_proto: std::collections::HashMap::new(),
        }
    }
}

/// A process-wide [`FetchCommands`] holding only the `make.globals`
/// defaults, for callers with no resolved config (`WgetFetcher`'s absent
/// override).
pub fn default_commands() -> &'static FetchCommands {
    static DEFAULT: std::sync::LazyLock<FetchCommands> =
        std::sync::LazyLock::new(FetchCommands::default);
    &DEFAULT
}

/// Portage `fetch.py`'s non-path substitution inputs: the
/// distfile's `DIGESTS` string and the `PORTAGE_SSH_OPTS` setting.
/// `None` and `Some("")` expand identically (both to empty).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FetchCommandVars<'a> {
    /// `" ".join(f"{k.lower()}:{v}" ...)` over the Manifest hashes, size
    /// excluded -- build it with [`digests_variable`].
    pub digests: Option<&'a str>,
    /// Portage `mysettings.get("PORTAGE_SSH_OPTS")` (used by the shipped
    /// `FETCHCOMMAND_SSH`/`_SFTP` templates as `"${PORTAGE_SSH_OPTS}"`).
    pub portage_ssh_opts: Option<&'a str>,
}

/// The `DIGESTS` fetch-command variable for one Manifest entry (Portage
/// `fetch.py`): `" ".join(f"{k.lower()}:{v}")` over every hash
/// (the `size` entry excluded), e.g. `blake2b:<hex> sha512:<hex>`.
/// Names are sorted (see notes); the shipped tree's order matches the sort.
pub fn digests_variable(digests: &DistfileDigests) -> String {
    let mut names: Vec<&String> = digests.hashes.keys().collect();
    names.sort();
    names
        .iter()
        .map(|name| format!("{}:{}", name.to_ascii_lowercase(), digests.hashes[*name]))
        .collect::<Vec<_>>()
        .join(" ")
}

impl FetchCommands {
    /// Portage's selection (Portage `fetch.py`): the protocol variant
    /// first, then the plain name.
    fn select(&self, proto: &str, resume: bool) -> Result<(String, String), String> {
        let (plain_var, plain, proto_var, proto_map) = if resume {
            (
                "RESUMECOMMAND",
                &self.resumecommand,
                format!("RESUMECOMMAND_{proto}"),
                &self.resumecommand_proto,
            )
        } else {
            (
                "FETCHCOMMAND",
                &self.fetchcommand,
                format!("FETCHCOMMAND_{proto}"),
                &self.fetchcommand_proto,
            )
        };
        if let Some(cmd) = proto_map.get(proto) {
            return Ok((proto_var, cmd.clone()));
        }
        match plain {
            Some(cmd) => Ok((plain_var.to_string(), cmd.clone())),
            None => Err(format!(
                "!!! {plain_var} is unset. It should have been defined in\n\
                 !!! /usr/share/portage/config/make.globals.\n"
            )),
        }
    }
}

/// Portage `varexpand` + `shlex.split` over the selected command
/// (Portage `fetch.py`): variables are substituted, then the string is
/// split into argv with POSIX quoting rules -- never run through a shell.
pub fn expand_and_split(command: &str, vars: &HashMap<String, String>) -> Vec<String> {
    shell_split(&portage_util::varexpand(command, |name| {
        vars.get(name).cloned()
    }))
}

/// POSIX-ish argv splitting, public for callers parsing a config value
/// Portage runs through `shlex.split` (e.g. `PORTAGE_RO_DISTDIRS`).
pub fn split_shell_words(s: &str) -> Vec<String> {
    shell_split(s)
}

/// Python `shlex.split` (posix) argv splitting: whitespace separates,
/// single quotes are literal, a backslash escapes the next character
/// outside single quotes, and inside double quotes only `\"`/`\\` lose
/// the backslash.
fn shell_split(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut has_cur = false;
    let mut chars = s.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(c) = chars.next() {
        if in_single {
            if c == '\'' {
                in_single = false;
            } else {
                cur.push(c);
            }
            has_cur = true;
            continue;
        }
        if in_double {
            if c == '"' {
                in_double = false;
            } else if c == '\\'
                && let Some(&next) = chars.peek()
                && matches!(next, '"' | '\\')
            {
                // Python `shlex` (posix) removes the backslash only before
                // the closing quote and before another backslash.
                cur.push(chars.next().unwrap());
            } else {
                cur.push(c);
            }
            has_cur = true;
            continue;
        }
        match c {
            ' ' | '\t' | '\n' => {
                if has_cur {
                    args.push(std::mem::take(&mut cur));
                    has_cur = false;
                }
            }
            '\'' => {
                in_single = true;
                has_cur = true;
            }
            '"' => {
                in_double = true;
                has_cur = true;
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                    has_cur = true;
                }
            }
            _ => {
                cur.push(c);
                has_cur = true;
            }
        }
    }
    if has_cur {
        args.push(cur);
    }
    args
}

/// Portage `fetch.py`: select `FETCHCOMMAND`
/// (or its protocol variant) and `RESUMECOMMAND`, substitute the
/// variables, and spawn the subprocess (never an in-process HTTP
/// client). A failed fresh fetch removes its partial; a failed resume
/// keeps it for the next candidate.
///
/// The `${FILE}` refusal is Portage's (see notes): each command missing
/// the parameter is reported, and the fetch aborts only when the
/// distfile name differs from the URL basename.
///
/// `FILE` is `dest`'s own basename, **not** Portage's
/// `basename(download_path)` (see notes).
/// The fetch command's selection + `${FILE}` check + substitution reduce
/// to: the make.conf variable name to blame, the program, and its argv
/// -- everything short of the spawn itself, so the inheriting transport
/// ([`download_with_commands`]) and the capturing one
/// ([`download_with_commands_quiet`]) run byte-identical command lines
/// and differ only in where the child's output goes.
struct PlannedFetch {
    prog: String,
    args: Vec<String>,
    /// Portage's `${FILE}`-less-command warning (emitted when the
    /// distfile name matched the URL basename so Portage runs the
    /// command anyway). The caller decides where it goes: stderr for the
    /// inheriting transport, the captured stream for the quiet one.
    file_param_warning: Option<String>,
}
/// Arguments of [`plan_fetch_command`].
struct PlanFetchCommandArgs<'a> {
    uri: &'a str,
    dest: &'a Path,
    resume: bool,
    distdir: &'a Path,
    vars: FetchCommandVars<'a>,
    commands: &'a FetchCommands,
}

fn plan_fetch_command(args: PlanFetchCommandArgs<'_>) -> Result<PlannedFetch, String> {
    let PlanFetchCommandArgs {
        uri,
        dest,
        resume,
        distdir,
        vars,
        commands,
    } = args;
    let proto = uri
        .split_once("://")
        .map(|(p, _)| p.to_ascii_uppercase())
        .unwrap_or_default();
    let (fetch_var, fetch_command) = commands.select(&proto, false)?;
    let (resume_var, resume_command) = commands.select(&proto, true)?;
    let file = dest
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let missing_file_param: String = [(&fetch_var, &fetch_command), (&resume_var, &resume_command)]
        .iter()
        .filter(|(_, command)| !command.contains("${FILE}"))
        .map(|(var, _)| format!("!!! {var} does not contain the required ${{FILE}} parameter.\n"))
        .collect();
    let mut file_param_warning = None;
    if !missing_file_param.is_empty() {
        let hint = "!!! Refer to the make.conf(5) man page for information about how to\n\
                    !!! correctly specify FETCHCOMMAND and RESUMECOMMAND.\n";
        let message = format!("{missing_file_param}{hint}");
        let url_basename = uri.rsplit('/').next().unwrap_or("");
        if file != url_basename {
            // Portage `return 0`: abort the fetch, never spawn.
            return Err(message);
        }
        // Names match: Portage still prints the warning before running
        // the command; there is no `Err` to carry it here, so the caller
        // emits it on the transport's own channel.
        file_param_warning = Some(message);
    }
    let (var, command) = if resume {
        (resume_var, resume_command)
    } else {
        (fetch_var, fetch_command)
    };
    let mut variables: HashMap<String, String> = HashMap::new();
    variables.insert("URI".to_string(), uri.to_string());
    variables.insert("FILE".to_string(), file.clone());
    variables.insert("DISTDIR".to_string(), distdir.display().to_string());
    if let Some(digests) = vars.digests {
        variables.insert("DIGESTS".to_string(), digests.to_string());
    }
    if let Some(opts) = vars.portage_ssh_opts {
        variables.insert("PORTAGE_SSH_OPTS".to_string(), opts.to_string());
    }
    let argv = expand_and_split(&command, &variables);
    let Some((prog, rest)) = argv.split_first() else {
        return Err(format!("!!! {var} is empty.\n"));
    };
    Ok(PlannedFetch {
        prog: prog.clone(),
        args: rest.to_vec(),
        file_param_warning,
    })
}
/// Arguments of [`download_with_commands`].
pub struct DownloadWithCommandsArgs<'a> {
    pub uri: &'a str,
    pub dest: &'a Path,
    pub resume: bool,
    pub distdir: &'a Path,
    pub vars: FetchCommandVars<'a>,
    pub commands: &'a FetchCommands,
}

pub fn download_with_commands(args: DownloadWithCommandsArgs<'_>) -> Result<(), String> {
    let DownloadWithCommandsArgs {
        uri,
        dest,
        resume,
        distdir,
        vars,
        commands,
    } = args;
    let planned = plan_fetch_command(PlanFetchCommandArgs {
        uri,
        dest,
        resume,
        distdir,
        vars,
        commands,
    })?;
    if let Some(warning) = planned.file_param_warning {
        // Names match: Portage still prints the warning before running
        // the command; there is no `Err` to carry it here, so it goes to
        // stderr.
        eprint!("{warning}");
    }
    let status = std::process::Command::new(&planned.prog)
        .args(&planned.args)
        .status()
        .map_err(|e| format!("failed to spawn {}: {e}", planned.prog))?;
    if !status.success() {
        if !resume {
            let _ = std::fs::remove_file(dest);
        }
        return Err(format!(
            "{} failed to fetch {uri:?} ({status})",
            planned.prog
        ));
    }
    Ok(())
}

/// [`download_with_commands`], with the child's stderr sent to stdout like
/// `BinpkgFetcher` does (`fd_pipes[2] = sys.__stdout__`): wget writes its
/// transcript to stderr and Portage shows it on stdout.
// Portage: _emerge/BinpkgFetcher.py, _start
pub fn download_with_commands_stdout(args: DownloadWithCommandsArgs<'_>) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::fd::AsFd as _;
    let DownloadWithCommandsArgs {
        uri,
        dest,
        resume,
        distdir,
        vars,
        commands,
    } = args;
    let planned = plan_fetch_command(PlanFetchCommandArgs {
        uri,
        dest,
        resume,
        distdir,
        vars,
        commands,
    })?;
    if let Some(warning) = planned.file_param_warning {
        eprint!("{warning}");
    }
    let _ = std::io::stdout().flush();
    let out = std::io::stdout()
        .as_fd()
        .try_clone_to_owned()
        .map_err(|e| format!("stdout: {e}"))?;
    let status = std::process::Command::new(&planned.prog)
        .args(&planned.args)
        .stderr(std::process::Stdio::from(out))
        .status()
        .map_err(|e| format!("failed to spawn {}: {e}", planned.prog))?;
    if !status.success() {
        if !resume {
            let _ = std::fs::remove_file(dest);
        }
        return Err(format!(
            "{} failed to fetch {uri:?} ({status})",
            planned.prog
        ));
    }
    Ok(())
}

/// A quiet fetch failure: the same one-line summary
/// [`download_with_commands`] would return, plus everything the fetch
/// command wrote to stderr (its stdout is discarded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuietFetchError {
    pub summary: String,
    pub stderr: String,
}

/// Arguments of [`download_with_commands_quiet`].
pub struct DownloadWithCommandsQuietArgs<'a> {
    pub uri: &'a str,
    pub dest: &'a Path,
    pub resume: bool,
    pub distdir: &'a Path,
    pub vars: FetchCommandVars<'a>,
    pub commands: &'a FetchCommands,
}

/// The capturing sibling of [`download_with_commands`]: the same command
/// selection, `${FILE}` check, and substitution, but the child's stdout
/// is dropped and its stderr captured -- never inherited. For callers
/// whose failure message is shaped from the transcript (Portage
/// `bintree._populate_remote`'s `!!! [repo] Error fetching ...` shape).
pub fn download_with_commands_quiet(
    args: DownloadWithCommandsQuietArgs<'_>,
) -> Result<(), QuietFetchError> {
    let DownloadWithCommandsQuietArgs {
        uri,
        dest,
        resume,
        distdir,
        vars,
        commands,
    } = args;
    let planned = plan_fetch_command(PlanFetchCommandArgs {
        uri,
        dest,
        resume,
        distdir,
        vars,
        commands,
    })
    .map_err(|summary| QuietFetchError {
        summary,
        stderr: String::new(),
    })?;
    let output = std::process::Command::new(&planned.prog)
        .args(&planned.args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| QuietFetchError {
            summary: format!("failed to spawn {}: {e}", planned.prog),
            stderr: planned.file_param_warning.clone().unwrap_or_default(),
        })?;
    if !output.status.success() {
        if !resume {
            let _ = std::fs::remove_file(dest);
        }
        let mut stderr = planned.file_param_warning.clone().unwrap_or_default();
        stderr.push_str(&String::from_utf8_lossy(&output.stderr));
        return Err(QuietFetchError {
            summary: format!(
                "{} failed to fetch {uri:?} ({})",
                planned.prog, output.status
            ),
            stderr,
        });
    }
    Ok(())
}

/// The `make.globals` default transport, for callers with no resolved
/// config: `download_with_commands` over the defaults, no extra
/// variables. Shared by the `portuale` fetch path and the `mrg-director`
/// `Fetcher` seam so the transport is never duplicated per caller.
pub fn download_via_wget(uri: &str, dest: &Path, resume: bool) -> Result<(), String> {
    let distdir = dest.parent().unwrap_or_else(|| Path::new("."));
    download_with_commands(DownloadWithCommandsArgs {
        uri,
        dest,
        resume,
        distdir,
        vars: FetchCommandVars::default(),
        commands: &FetchCommands::default(),
    })
}

/// The quiet `make.globals` default transport: [`download_via_wget`]'s
/// own command line, but output-captured ([`download_with_commands_quiet`])
/// -- for callers that shape the failure themselves (a binhost index
/// refresh prints Portage's `!!! [repo] ...` lines, never wget's raw
/// transcript).
pub fn download_via_wget_quiet(uri: &str, dest: &Path) -> Result<(), QuietFetchError> {
    let distdir = dest.parent().unwrap_or_else(|| Path::new("."));
    download_with_commands_quiet(DownloadWithCommandsQuietArgs {
        uri,
        dest,
        resume: false,
        distdir,
        vars: FetchCommandVars::default(),
        commands: &FetchCommands::default(),
    })
}

/// Portage digest verification: file size first (Portage `_check_distfile`'s
/// cheap pre-check before hashing) plus every `BLAKE2B`/`SHA512` entry
/// `digests` carries (see notes for why only those two). Any other hash
/// name is silently skipped. An empty `digests.hashes` still passes once
/// the size matches (Portage's "size alone is still something" tolerance).
pub fn verify_digests(path: &Path, digests: &DistfileDigests) -> Result<(), Error> {
    // `blake2`/`sha2` re-export the same `digest::Digest` trait under
    // their own names -- one import covers both hashers below.
    use blake2::Digest as _;

    let bytes = std::fs::read(path).map_err(|e| Error::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    if bytes.len() as u64 != digests.size {
        return Err(Error::SizeMismatch {
            path: path.display().to_string(),
            expected: digests.size,
            got: bytes.len(),
        });
    }
    for (algo, expected) in &digests.hashes {
        let actual = match algo.as_str() {
            "BLAKE2B" => to_hex(&blake2::Blake2b512::digest(&bytes)),
            "SHA512" => to_hex(&sha2::Sha512::digest(&bytes)),
            _ => continue,
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(Error::HashMismatch {
                path: path.display().to_string(),
                algo: algo.to_string(),
                expected: expected.to_string(),
                actual,
            });
        }
    }
    Ok(())
}

/// The streaming sibling of [`verify_digests`]: the same size-first check
/// and the same BLAKE2B/SHA512 set, but the bytes come from a reader --
/// the gpkg outer-container member stream, so a large member is neither
/// buffered nor copied to scratch. `name` labels the member in errors.
pub fn verify_digests_reader<R: std::io::Read>(
    name: &str,
    mut reader: R,
    digests: &DistfileDigests,
) -> Result<(), Error> {
    // `blake2`/`sha2` re-export the same `digest::Digest` trait; see
    // `verify_digests`.
    use blake2::Digest as _;

    let mut blake2b = blake2::Blake2b512::new();
    let mut sha512 = sha2::Sha512::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size: u64 = 0;
    loop {
        let n = reader.read(&mut buffer).map_err(|source| Error::Io {
            path: name.to_string(),
            source,
        })?;
        if n == 0 {
            break;
        }
        size += n as u64;
        blake2b.update(&buffer[..n]);
        sha512.update(&buffer[..n]);
    }
    if size != digests.size {
        return Err(Error::SizeMismatch {
            path: name.to_string(),
            expected: digests.size,
            got: size as usize,
        });
    }
    let blake2b_hex = to_hex(&blake2b.finalize());
    let sha512_hex = to_hex(&sha512.finalize());
    for (algo, expected) in &digests.hashes {
        let actual = match algo.as_str() {
            "BLAKE2B" => blake2b_hex.clone(),
            "SHA512" => sha512_hex.clone(),
            _ => continue,
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(Error::HashMismatch {
                path: name.to_string(),
                algo: algo.to_string(),
                expected: expected.to_string(),
                actual,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;
    use std::fs;

    /// Serialises stub-script creation against every stub spawn across
    /// the parallel test threads (see notes for the `ETXTBSY` story).
    static SPAWN_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn spawn_guard() -> std::sync::MutexGuard<'static, ()> {
        // Recover from poisoning: a panicking test must not cascade into
        // every other spawn test.
        SPAWN_SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn tempdir() -> std::path::PathBuf {
        // Unique per call across parallel test threads (pid + counter);
        // the timestamp is a run marker only (see notes).
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = TempDir::new(&format!("portage_fetch_test_{}_{}", std::process::id(), n)).keep();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn fetch_commands_select_the_protocol_variant_then_the_plain_name() {
        let mut commands = FetchCommands::default();
        commands
            .fetchcommand_proto
            .insert("HTTP".to_string(), "curl ${URI}".to_string());
        commands.fetchcommand = Some("wget ${FILE}".to_string());
        assert_eq!(
            commands.select("HTTP", false).unwrap(),
            ("FETCHCOMMAND_HTTP".to_string(), "curl ${URI}".to_string())
        );
        assert_eq!(
            commands.select("HTTPS", false).unwrap(),
            ("FETCHCOMMAND".to_string(), "wget ${FILE}".to_string())
        );
        // Resume picks the RESUMECOMMAND family.
        commands
            .resumecommand_proto
            .insert("HTTPS".to_string(), "curl -C - ${URI}".to_string());
        assert_eq!(
            commands.select("HTTPS", true).unwrap(),
            (
                "RESUMECOMMAND_HTTPS".to_string(),
                "curl -C - ${URI}".to_string()
            )
        );
        // A default-constructed family selects the shipped make.globals
        // templates: an absent config key means "default", never "unset".
        let defaults = FetchCommands::default();
        assert_eq!(
            defaults.select("HTTPS", false).unwrap(),
            ("FETCHCOMMAND".to_string(), DEFAULT_FETCHCOMMAND.to_string())
        );
        assert_eq!(
            defaults.select("HTTPS", true).unwrap(),
            (
                "RESUMECOMMAND".to_string(),
                DEFAULT_RESUMECOMMAND.to_string()
            )
        );
        // An unset plain command is Portage's own error, reachable only
        // from a hand-built family.
        let empty = FetchCommands {
            fetchcommand: None,
            resumecommand: None,
            ..FetchCommands::default()
        };
        let err = empty.select("HTTPS", false).unwrap_err();
        assert!(err.contains("FETCHCOMMAND is unset"), "{err}");
    }

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn expand_and_split_substitutes_and_quotes_like_shlex() {
        let argv = expand_and_split(
            "wget -O \"${DISTDIR}/${FILE}\" \"${URI}\" -U 'Mozilla x'",
            &vars(&[
                ("DISTDIR", "/var/cache/distfiles"),
                ("FILE", "foo-1.0.tar.gz"),
                ("URI", "https://example.com/foo-1.0.tar.gz"),
            ]),
        );
        assert_eq!(
            argv,
            vec![
                "wget",
                "-O",
                "/var/cache/distfiles/foo-1.0.tar.gz",
                "https://example.com/foo-1.0.tar.gz",
                "-U",
                "Mozilla x",
            ]
        );
        // The bare `$VAR` form is Portage `varexpand`'s too.
        let argv = expand_and_split(
            "cp $URI ${DISTDIR}/${FILE}",
            &vars(&[
                ("URI", "file:///src/foo"),
                ("DISTDIR", "/d"),
                ("FILE", "foo"),
            ]),
        );
        assert_eq!(argv, vec!["cp", "file:///src/foo", "/d/foo"]);
    }

    /// Oracle: Portage's `FETCHCOMMAND_SSH` expanded with the fetch
    /// variables (see notes for the verbatim vectors).
    #[test]
    fn expand_and_split_matches_portage_varexpand_on_fetchcommand_ssh() {
        let command = r#"bash -c "x=\${2#ssh://} ; host=\${x%%/*} ; port=\${host##*:} ; host=\${host%:*} ; [[ \${host} = \${port} ]] && port= ; exec rsync --rsh=\"ssh \${port:+-p\${port}} \${3}\" -avP \"\${host}:/\${x#*/}\" \"\$1\"" rsync "${DISTDIR}/${FILE}" "${URI}" "${PORTAGE_SSH_OPTS}""#;
        let base = vars(&[
            ("DISTDIR", "/var/cache/distfiles"),
            ("URI", "https://example.org/distfiles/foo-1.0.tar.gz"),
            ("FILE", "foo-1.0.tar.gz"),
            ("DIGESTS", "blake2b:aa sha512:bb"),
        ]);
        let bash_script = r#"x=${2#ssh://} ; host=${x%%/*} ; port=${host##*:} ; host=${host%:*} ; [[ ${host} = ${port} ]] && port= ; exec rsync --rsh="ssh ${port:+-p${port}} ${3}" -avP "${host}:/${x#*/}" "$1""#;
        let mut unset = base.clone();
        assert_eq!(
            expand_and_split(command, &unset),
            vec![
                "bash",
                "-c",
                bash_script,
                "rsync",
                "/var/cache/distfiles/foo-1.0.tar.gz",
                "https://example.org/distfiles/foo-1.0.tar.gz",
                "",
            ]
        );
        unset.insert(
            "PORTAGE_SSH_OPTS".to_string(),
            "-o ServerAliveInterval=5 -o User=portage".to_string(),
        );
        assert_eq!(
            expand_and_split(command, &unset),
            vec![
                "bash",
                "-c",
                bash_script,
                "rsync",
                "/var/cache/distfiles/foo-1.0.tar.gz",
                "https://example.org/distfiles/foo-1.0.tar.gz",
                "-o ServerAliveInterval=5 -o User=portage",
            ]
        );
    }

    /// Portage fetch-time `varexpand` over a command template: unknown
    /// names empty, `\$` literal, `\\` unescaped, single-quote suspension.
    #[test]
    fn expand_and_split_varexpand_unknown_vars_and_escapes_match_portage() {
        assert_eq!(
            expand_and_split("cp $NOSUCH ${ALSO_MISSING} x", &vars(&[("FILE", "f")])),
            vec!["cp", "x"]
        );
        assert_eq!(
            expand_and_split(
                r#"cp "\$FILE" "\${URI}" "\\$FILE""#,
                &vars(&[("FILE", "f"), ("URI", "u")]),
            ),
            vec!["cp", "$FILE", "${URI}", "\\$FILE"]
        );
        assert_eq!(
            expand_and_split("cp '$FILE' x", &vars(&[("FILE", "f")])),
            vec!["cp", "$FILE", "x"]
        );
    }

    #[test]
    fn digests_variable_formats_like_portage_and_skips_size() {
        let mut hashes = HashMap::new();
        hashes.insert("SHA512".to_string(), "bb".to_string());
        hashes.insert("BLAKE2B".to_string(), "aa".to_string());
        let digests = DistfileDigests { size: 11, hashes };
        assert_eq!(digests_variable(&digests), "blake2b:aa sha512:bb");
    }

    /// A stub "fetch command": `cp <uri-path> <dest>`. Spawned directly,
    /// exactly the way a Portage `FETCHCOMMAND` is.
    fn stub_fetch_script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        // Under the spawn lock: no other test may fork (spawn) while
        // this write-fd is open — see `SPAWN_SERIAL`.
        let _guard = spawn_guard();
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    /// Arguments of [`download_serialized`].
    struct DownloadSerializedArgs<'a> {
        uri: &'a str,
        dest: &'a std::path::Path,
        resume: bool,
        distdir: &'a std::path::Path,
        vars: FetchCommandVars<'a>,
        commands: &'a FetchCommands,
    }

    /// The only way these tests spawn a stub: [`download_with_commands`]
    /// under the spawn lock (see `SPAWN_SERIAL`).
    fn download_serialized(args: DownloadSerializedArgs<'_>) -> Result<(), String> {
        let DownloadSerializedArgs {
            uri,
            dest,
            resume,
            distdir,
            vars,
            commands,
        } = args;
        let _guard = spawn_guard();
        download_with_commands(DownloadWithCommandsArgs {
            uri,
            dest,
            resume,
            distdir,
            vars,
            commands,
        })
    }

    #[test]
    fn download_with_commands_runs_a_stub_command_and_refuses_one_without_file() {
        let dir = tempdir();
        let source = dir.join("source.tar.gz");
        fs::write(&source, b"payload").unwrap();
        let dest = dir.join("dest.tar.gz");
        let script = stub_fetch_script(&dir, "fetch.sh", "cp \"${2#file://}\" \"$1\"");
        let mut commands = FetchCommands {
            fetchcommand: Some(format!(
                "{} \"${{DISTDIR}}/${{FILE}}\" \"${{URI}}\"",
                script.display()
            )),
            ..FetchCommands::default()
        };
        let uri = format!("file://{}", source.display());
        download_serialized(DownloadSerializedArgs {
            uri: &uri,
            dest: &dest,
            resume: false,
            distdir: &dir,
            vars: FetchCommandVars::default(),
            commands: &commands,
        })
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"payload");

        // A command without ${FILE} is refused with Portage's message and
        // never spawned.
        commands.fetchcommand = Some(format!("{} \"${{URI}}\"", script.display()));
        let err = download_serialized(DownloadSerializedArgs {
            uri: &uri,
            dest: &dest,
            resume: false,
            distdir: &dir,
            vars: FetchCommandVars::default(),
            commands: &commands,
        })
        .unwrap_err();
        assert!(
            err.contains("does not contain the required ${FILE} parameter"),
            "{err}"
        );

        // The protocol variant wins for a file:// URI.
        commands.fetchcommand_proto.insert(
            "FILE".to_string(),
            format!(
                "{} \"${{DISTDIR}}/${{FILE}}\" \"${{URI}}\"",
                script.display()
            ),
        );
        commands.fetchcommand = Some("false".to_string());
        let dest2 = dir.join("dest2.tar.gz");
        download_serialized(DownloadSerializedArgs {
            uri: &uri,
            dest: &dest2,
            resume: false,
            distdir: &dir,
            vars: FetchCommandVars::default(),
            commands: &commands,
        })
        .unwrap();
        assert!(dest2.exists());
    }

    /// Portage checks both fetch and resume commands for `${FILE}`
    /// and aborts only when the distfile name differs from the URL
    /// basename (see notes).
    #[test]
    fn download_with_commands_warns_for_both_commands_and_aborts_only_when_renamed() {
        let srcdir = tempdir();
        let dldir = tempdir();
        let source = srcdir.join("source.tar.gz");
        fs::write(&source, b"payload").unwrap();
        let marker = dldir.join("spawned");
        let script = stub_fetch_script(
            &dldir,
            "wget-p.sh",
            // The stub of `wget -P "${DISTDIR}" "${URI}"`: copy the URI's
            // basename into the directory, no ${FILE} anywhere.
            &format!("touch {}\ncp \"${{2#file://}}\" \"$1/\"", marker.display()),
        );
        let uri = format!("file://{}", source.display());
        let no_file = format!("{} \"${{DISTDIR}}\" \"${{URI}}\"", script.display());

        // (a) the distfile name equals the URL basename: the warning is
        // printed to stderr and the command still runs.
        let dest = dldir.join("source.tar.gz");
        let commands = FetchCommands {
            fetchcommand: Some(no_file.clone()),
            ..FetchCommands::default()
        };
        download_serialized(DownloadSerializedArgs {
            uri: &uri,
            dest: &dest,
            resume: false,
            distdir: &dldir,
            vars: FetchCommandVars::default(),
            commands: &commands,
        })
        .unwrap();
        assert!(marker.is_file(), "the ${{FILE}}-less command ran");
        assert_eq!(fs::read(&dest).unwrap(), b"payload");
        fs::remove_file(&marker).unwrap();

        // (b) a renamed distfile differs from the URL basename: Portage
        // aborts before spawning.
        let renamed = dldir.join("renamed.tar.gz");
        let err = download_serialized(DownloadSerializedArgs {
            uri: &uri,
            dest: &renamed,
            resume: false,
            distdir: &dldir,
            vars: FetchCommandVars::default(),
            commands: &commands,
        })
        .unwrap_err();
        assert!(
            err.contains("FETCHCOMMAND does not contain the required ${FILE} parameter"),
            "{err}"
        );
        assert!(err.contains("make.conf(5)"), "{err}");
        assert!(!marker.exists(), "no spawn after the abort");
        assert!(!renamed.exists());

        // (c) only RESUMECOMMAND lacks ${FILE}: it is selected and named
        // even though this is a fresh fetch, and it aborts the renamed
        // case.
        let commands = FetchCommands {
            resumecommand: Some(no_file),
            ..FetchCommands::default()
        };
        let err = download_serialized(DownloadSerializedArgs {
            uri: &uri,
            dest: &renamed,
            resume: false,
            distdir: &dldir,
            vars: FetchCommandVars::default(),
            commands: &commands,
        })
        .unwrap_err();
        assert!(
            err.contains("RESUMECOMMAND does not contain the required ${FILE} parameter"),
            "{err}"
        );
        assert!(!marker.exists(), "no spawn after the abort");
    }

    #[test]
    fn download_with_commands_removes_a_fresh_partial_but_keeps_a_resumed_one() {
        let dir = tempdir();
        let dest = dir.join("dest.tar.gz");
        let script = stub_fetch_script(&dir, "fail.sh", "echo partial > \"$1\"; exit 1");
        let mut commands = FetchCommands {
            fetchcommand: Some(format!(
                "{} \"${{DISTDIR}}/${{FILE}}\" \"${{URI}}\"",
                script.display()
            )),
            ..FetchCommands::default()
        };
        commands.resumecommand = commands.fetchcommand.clone();
        download_serialized(DownloadSerializedArgs {
            uri: "file:///nonexistent",
            dest: &dest,
            resume: false,
            distdir: &dir,
            vars: FetchCommandVars::default(),
            commands: &commands,
        })
        .unwrap_err();
        assert!(!dest.exists(), "a failed fresh fetch removes the partial");
        download_serialized(DownloadSerializedArgs {
            uri: "file:///nonexistent",
            dest: &dest,
            resume: true,
            distdir: &dir,
            vars: FetchCommandVars::default(),
            commands: &commands,
        })
        .unwrap_err();
        assert!(
            dest.exists(),
            "a failed resume keeps it for the next candidate"
        );
    }

    #[test]
    fn parse_manifest_reads_a_real_dist_line_with_both_hashes() {
        let dir = tempdir();
        let manifest = dir.join("Manifest");
        fs::write(
            &manifest,
            "DIST fuse-3.18.2.tar.gz 4933779 BLAKE2B aaaa SHA512 bbbb\n\
             EBUILD fuse-3.18.2.ebuild 123 BLAKE2B cccc SHA512 dddd\n",
        )
        .unwrap();
        let parsed = parse_manifest(&manifest).unwrap();
        assert_eq!(parsed.len(), 1, "EBUILD lines must not be parsed as DIST");
        let entry = &parsed["fuse-3.18.2.tar.gz"];
        assert_eq!(entry.size, 4933779);
        assert_eq!(entry.hashes["BLAKE2B"], "aaaa");
        assert_eq!(entry.hashes["SHA512"], "bbbb");
    }

    #[test]
    fn parse_manifest_is_empty_for_a_missing_file() {
        let dir = tempdir();
        let parsed = parse_manifest(&dir.join("Manifest")).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn flatten_src_uri_handles_plain_uris_with_no_conditionals() {
        let entries = flatten_src_uri("https://example.com/a-1.0.tar.gz", |_, _| true).unwrap();
        assert_eq!(
            entries,
            vec![SrcUriEntry {
                uri: "https://example.com/a-1.0.tar.gz".to_string(),
                filename: "a-1.0.tar.gz".to_string(),
                override_mirror: false,
                override_fetch: false,
            }]
        );
    }

    #[test]
    fn flatten_src_uri_honors_the_arrow_rename() {
        let entries =
            flatten_src_uri("https://example.com/dl?id=1 -> a-1.0.tar.gz", |_, _| true).unwrap();
        assert_eq!(entries[0].filename, "a-1.0.tar.gz");
        assert_eq!(entries[0].uri, "https://example.com/dl?id=1");
    }

    #[test]
    fn flatten_src_uri_strips_a_mirror_prefix_and_records_both_overrides() {
        let entries =
            flatten_src_uri("mirror+https://example.com/a-1.0.tar.gz", |_, _| true).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].uri, "https://example.com/a-1.0.tar.gz");
        assert_eq!(entries[0].filename, "a-1.0.tar.gz");
        assert!(entries[0].override_mirror);
        assert!(entries[0].override_fetch, "mirror+ implies fetch+");
    }

    #[test]
    fn flatten_src_uri_strips_a_fetch_prefix_and_records_only_override_fetch() {
        let entries = flatten_src_uri(
            "fetch+https://example.com/dl?v=1 -> a-1.0.tar.gz",
            |_, _| true,
        )
        .unwrap();
        assert_eq!(entries[0].uri, "https://example.com/dl?v=1");
        assert_eq!(entries[0].filename, "a-1.0.tar.gz");
        assert!(!entries[0].override_mirror);
        assert!(entries[0].override_fetch);
    }

    #[test]
    fn flatten_src_uri_a_plain_uri_has_neither_override() {
        let entries = flatten_src_uri("https://example.com/a-1.0.tar.gz", |_, _| true).unwrap();
        assert!(!entries[0].override_mirror);
        assert!(!entries[0].override_fetch);
    }

    #[test]
    fn flatten_src_uri_includes_a_positive_conditional_only_when_active() {
        let src = "unconditional-1.0.tar.gz test? ( test-only-1.0.tar.gz )";
        let with_test = flatten_src_uri(src, |negated, flag| !negated && flag == "test").unwrap();
        assert_eq!(with_test.len(), 2);

        let without_test = flatten_src_uri(src, |_, _| false).unwrap();
        assert_eq!(without_test.len(), 1);
        assert_eq!(without_test[0].filename, "unconditional-1.0.tar.gz");
    }

    #[test]
    fn flatten_src_uri_negated_conditional_is_active_when_the_flag_is_unset() {
        // Portuale's always-empty USE set means every `!flag?` group is
        // active -- the common way an optional dependency's SRC_URI still
        // gets fetched by default.
        let is_set = |_flag: &str| false;
        let active = |negated: bool, flag: &str| {
            if negated { !is_set(flag) } else { is_set(flag) }
        };
        let entries = flatten_src_uri("!test? ( a-1.0.tar.gz )", active).unwrap();
        assert_eq!(entries.len(), 1);

        let entries = flatten_src_uri("test? ( a-1.0.tar.gz )", active).unwrap();
        assert_eq!(
            entries.len(),
            0,
            "a positive conditional on an unset flag must not fire"
        );
    }

    #[test]
    fn flatten_src_uri_supports_nested_groups() {
        let src = "outer? ( a-1.0.tar.gz inner? ( b-1.0.tar.gz ) )";
        let all_active = flatten_src_uri(src, |_, _| true).unwrap();
        assert_eq!(all_active.len(), 2);

        let outer_only = flatten_src_uri(src, |_, flag| flag == "outer").unwrap();
        assert_eq!(outer_only.len(), 1);
        assert_eq!(outer_only[0].filename, "a-1.0.tar.gz");
    }

    #[test]
    fn flatten_src_uri_rejects_a_double_bar_any_of_group() {
        // Portage's SRC_URI grammar has no `||` at all (PMS 8.2.6.5):
        // `"||"` is an ordinary URI token, and `"("` with no preceding
        // `"flag?"` is the syntax error.
        let entries = flatten_src_uri("|| ( a-1.0.tar.gz )", |_, _| true);
        assert!(
            entries.is_err(),
            "\"(\" with no preceding \"flag?\" token is a real syntax error"
        );
    }

    #[test]
    fn flatten_src_uri_reports_an_unterminated_group() {
        assert!(flatten_src_uri("test? ( a-1.0.tar.gz", |_, _| true).is_err());
    }

    #[test]
    fn flatten_src_uri_reports_a_dangling_arrow() {
        assert!(flatten_src_uri("https://example.com/a ->", |_, _| true).is_err());
    }

    #[test]
    fn verify_digests_accepts_a_real_matching_file() {
        let dir = tempdir();
        let path = dir.join("hello.txt");
        fs::write(&path, b"hello world").unwrap();
        // Standard BLAKE2b-512 / SHA-512 test vectors for "hello world".
        let mut hashes = HashMap::new();
        hashes.insert(
            "BLAKE2B".to_string(),
            "021ced8799296ceca557832ab941a50b4a11f83478cf141f51f933f653ab9fbcc05a037cddbed06e309bf334942c4e58cdf1a46e237911ccd7fcf9787cbc7fd0".to_string(),
        );
        hashes.insert(
            "SHA512".to_string(),
            "309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f".to_string(),
        );
        let digests = DistfileDigests { size: 11, hashes };
        assert!(verify_digests(&path, &digests).is_ok());
    }

    #[test]
    fn verify_digests_rejects_a_size_mismatch_without_hashing() {
        let dir = tempdir();
        let path = dir.join("hello.txt");
        fs::write(&path, b"hello world").unwrap();
        let digests = DistfileDigests {
            size: 999,
            hashes: HashMap::new(),
        };
        let err = verify_digests(&path, &digests).unwrap_err();
        assert!(err.to_string().contains("size mismatch"), "{err}");
    }

    #[test]
    fn verify_digests_rejects_a_hash_mismatch() {
        let dir = tempdir();
        let path = dir.join("hello.txt");
        fs::write(&path, b"hello world").unwrap();
        let mut hashes = HashMap::new();
        hashes.insert("SHA512".to_string(), "0".repeat(128));
        let digests = DistfileDigests { size: 11, hashes };
        let err = verify_digests(&path, &digests).unwrap_err();
        assert!(err.to_string().contains("SHA512 mismatch"), "{err}");
    }

    #[test]
    fn verify_digests_ignores_an_unrecognized_hash_name() {
        let dir = tempdir();
        let path = dir.join("hello.txt");
        fs::write(&path, b"hello world").unwrap();
        let mut hashes = HashMap::new();
        hashes.insert("MD5".to_string(), "not-even-hex".to_string());
        let digests = DistfileDigests { size: 11, hashes };
        assert!(verify_digests(&path, &digests).is_ok());
    }

    #[test]
    fn verify_digests_reader_matches_verify_digests_on_the_same_bytes() {
        // The gpkg outer-container path streams a member through this
        // variant; same "hello world" vectors as the file tests above.
        let mut hashes = HashMap::new();
        hashes.insert(
            "BLAKE2B".to_string(),
            "021ced8799296ceca557832ab941a50b4a11f83478cf141f51f933f653ab9fbcc05a037cddbed06e309bf334942c4e58cdf1a46e237911ccd7fcf9787cbc7fd0".to_string(),
        );
        hashes.insert(
            "SHA512".to_string(),
            "309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f".to_string(),
        );
        let digests = DistfileDigests { size: 11, hashes };
        assert!(verify_digests_reader("member", &b"hello world"[..], &digests).is_ok());
        // A size mismatch is reported before hashing.
        let wrong_size = DistfileDigests {
            size: 999,
            hashes: HashMap::new(),
        };
        let err = verify_digests_reader("member", &b"hello world"[..], &wrong_size).unwrap_err();
        assert!(err.to_string().contains("size mismatch"), "{err}");
        // A hash mismatch names the member and the algorithm.
        let mut hashes = HashMap::new();
        hashes.insert("SHA512".to_string(), "0".repeat(128));
        let digests = DistfileDigests { size: 11, hashes };
        let err = verify_digests_reader("member", &b"hello world"[..], &digests).unwrap_err();
        assert!(err.to_string().contains("member: SHA512 mismatch"), "{err}");
    }

    #[test]
    fn parse_thirdpartymirrors_reads_a_real_grabdict_style_file() {
        let dir = tempdir();
        let path = dir.join("thirdpartymirrors");
        fs::write(
            &path,
            "# a comment line, skipped entirely\n\
             gentoo\thttps://distfiles.gentoo.org/distfiles https://gentoo.osuosl.org/distfiles\n\
             \n\
             gnu https://ftp.gnu.org/gnu/ # trailing comment truncates the rest\n\
             bare-name-no-urls\n",
        )
        .unwrap();
        let mirrors = parse_thirdpartymirrors(&path).unwrap();
        assert_eq!(
            mirrors.get("gentoo").unwrap(),
            &vec![
                "https://distfiles.gentoo.org/distfiles".to_string(),
                "https://gentoo.osuosl.org/distfiles".to_string(),
            ]
        );
        assert_eq!(
            mirrors.get("gnu").unwrap(),
            &vec!["https://ftp.gnu.org/gnu/".to_string()]
        );
        assert!(
            !mirrors.contains_key("bare-name-no-urls"),
            "a name with zero URLs must be skipped, matching real grabdict's own empty=0 default"
        );
    }

    #[test]
    fn parse_thirdpartymirrors_is_empty_for_a_missing_file() {
        let dir = tempdir();
        let mirrors = parse_thirdpartymirrors(&dir.join("does-not-exist")).unwrap();
        assert!(mirrors.is_empty());
    }

    #[test]
    fn resolve_mirror_candidates_expands_every_root_under_the_named_mirror() {
        let mut mirrors = HashMap::new();
        mirrors.insert(
            "gentoo".to_string(),
            vec![
                "https://distfiles.gentoo.org/distfiles".to_string(),
                "https://gentoo.osuosl.org/distfiles/".to_string(),
            ],
        );
        let candidates = resolve_mirror_candidates(
            "mirror://gentoo/app-arch/foo-1.0.tar.gz",
            &HashMap::new(),
            &mirrors,
        );
        assert_eq!(
            candidates,
            vec![
                "https://distfiles.gentoo.org/distfiles/app-arch/foo-1.0.tar.gz".to_string(),
                "https://gentoo.osuosl.org/distfiles/app-arch/foo-1.0.tar.gz".to_string(),
            ]
        );
    }

    #[test]
    fn resolve_mirror_candidates_tries_custommirrors_before_thirdpartymirrors() {
        // Portage "Try user-defined mirrors first".
        let mut custommirrors = HashMap::new();
        custommirrors.insert(
            "gentoo".to_string(),
            vec!["https://my-local-mirror.example/distfiles".to_string()],
        );
        let mut thirdpartymirrors = HashMap::new();
        thirdpartymirrors.insert(
            "gentoo".to_string(),
            vec!["https://distfiles.gentoo.org/distfiles".to_string()],
        );
        let candidates = resolve_mirror_candidates(
            "mirror://gentoo/app-arch/foo-1.0.tar.gz",
            &custommirrors,
            &thirdpartymirrors,
        );
        assert_eq!(
            candidates,
            vec![
                "https://my-local-mirror.example/distfiles/app-arch/foo-1.0.tar.gz".to_string(),
                "https://distfiles.gentoo.org/distfiles/app-arch/foo-1.0.tar.gz".to_string(),
            ]
        );
    }

    #[test]
    fn resolve_mirror_candidates_is_empty_for_an_unknown_mirror_name() {
        let candidates = resolve_mirror_candidates(
            "mirror://unknown/foo.tar.gz",
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(candidates.is_empty());
    }

    #[test]
    fn resolve_mirror_candidates_is_empty_for_a_malformed_mirror_uri() {
        let candidates =
            resolve_mirror_candidates("mirror://gentoo", &HashMap::new(), &HashMap::new());
        assert!(candidates.is_empty());
    }

    #[test]
    fn resolve_mirror_candidates_returns_a_non_mirror_uri_unchanged() {
        let candidates = resolve_mirror_candidates(
            "https://example.com/foo.tar.gz",
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(
            candidates,
            vec!["https://example.com/foo.tar.gz".to_string()]
        );
    }
}
