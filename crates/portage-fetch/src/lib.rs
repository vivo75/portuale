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
