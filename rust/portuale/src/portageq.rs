// `portuale portageq`: native `has_version` / `best_version` (feat#157,
// backlog #305 S6.2).
//
// Real `bin/portageq` is a Python program that reads `var/db/pkg`
// directly, so on a database-backed ROOT it cannot see the installed
// packages. This applet answers the same two questions from the
// installed-package database behind `portage_vdb`, and nothing else:
// every other portageq command is refused by name.
//
// Behaviour contract: `docs/evidence/305-s6-portageq.md` ("Rules the
// native implementation must follow", rules 1-10), taken from real
// `bin/portageq:80-203` and `:1408-1425`. Where the captures table and
// the rules disagree, the rules win. Design: `docs/vdb_to_db.md` §10.
//
// Matching reuses the resolver's own installed-package matcher
// (`portage_repo::best_installed_match`, the `best_installed_matching`
// core): slot, sub-slot, version operators, `::repo`, USE deps against the
// installed `USE`. There is no second matcher here.
//
// Backend: `PORTUALE_VDB_BACKEND` + `PORTUALE_VDB_PATH` (exported by `mrg`
// to its children) select a database; `PORTUALE_VDB_ROOT` (also exported
// by `mrg`) names the ROOT that database belongs to. The database is
// registered only for that root: `has_version -b` asks about BROOT `/`
// while ROOT is a chroot, and that other root keeps the files layout.
// Without `PORTUALE_VDB_ROOT` the database is used for whatever <eroot>
// is given (a hand-set environment names the backend explicitly).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use portage_vdb::BackendKind;

const USAGE: &str = "usage: portuale portageq {has_version|best_version} <eroot> <atom>";

/// Exit code for "the helper itself failed" (cannot open the database,
/// ambiguous bare name). `phase-helpers.sh` treats anything but 0/1 from
/// `has_version` as fatal (`unexpected portageq exit code`), which is
/// right: 1 would read as "not installed".
const EX_INTERNAL: u8 = 4;
/// `os.EX_USAGE`, what real portageq exits with for a bad <eroot>.
const EX_USAGE: u8 = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cmd {
    HasVersion,
    BestVersion,
}

impl Cmd {
    fn name(self) -> &'static str {
        match self {
            Cmd::HasVersion => "has_version",
            Cmd::BestVersion => "best_version",
        }
    }
}

pub fn run(args: &[String]) -> ExitCode {
    let Some(first) = args.first().map(String::as_str) else {
        eprintln!("{USAGE}");
        return ExitCode::from(EX_USAGE);
    };
    let cmd = match first {
        "has_version" => Cmd::HasVersion,
        "best_version" => Cmd::BestVersion,
        "-h" | "--help" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => {
            eprintln!("not implemented by portuale portageq: {other}");
            return ExitCode::from(1);
        }
    };
    ExitCode::from(run_query(cmd, &args[1..]))
}

fn run_query(cmd: Cmd, rest: &[String]) -> u8 {
    // Real: `uses_eroot` only when an <eroot> argument is present; it must
    // be a directory (`portageq:1469-1475`).
    if let Some(eroot) = rest.first()
        && !Path::new(eroot).is_dir()
    {
        eprintln!("Not a directory: '{eroot}'");
        eprintln!("Run portageq with --help for info");
        return EX_USAGE;
    }
    // Rule 1.
    if rest.len() < 2 {
        println!("ERROR: insufficient parameters!");
        return 3;
    }
    let root = PathBuf::from(&rest[0]);
    let raw = rest[1].as_str();

    // Rule 3: strict iff EBUILD_PHASE is set; EAPI then decides.
    let strict = std::env::var_os("EBUILD_PHASE").is_some();
    let eapi: Option<String> = if strict {
        std::env::var("EAPI").ok()
    } else {
        None
    };
    let allow_repo = !strict || eapi_has_repo_deps(eapi.as_deref());

    let parsed = portage_dep::parse_atom(raw).filter(|a| a.repo.is_none() || allow_repo);

    // The atom string handed to the matcher, and its unevaluated original.
    let (atom, unevaluated): (String, Option<String>) = match parsed {
        Some(a) => {
            if strict && let Some(msg) = eapi_violation(&a, raw, eapi.as_deref()) {
                // Rule 5.
                eqawarn(&[format!("QA Notice: {}: {msg}", cmd.name())]);
            }
            // Rule 2: only when USE is present in the environment.
            let evaluated = match std::env::var("USE") {
                Ok(use_str) => {
                    let set: HashSet<String> =
                        use_str.split_whitespace().map(str::to_string).collect();
                    portage_dep::evaluate_atom_conditionals(raw, &set)
                        .unwrap_or_else(|| raw.to_string())
                }
                Err(_) => raw.to_string(),
            };
            (evaluated, Some(raw.to_string()))
        }
        None if strict => {
            // Rule 4.
            eprintln!("ERROR: Invalid atom: '{raw}'");
            return 2;
        }
        None => {
            // Rule 6: the raw string goes to `vardb.match()`, which runs
            // `dep_expand` over it.
            if let Err(e) = setup_backend(&root) {
                eprintln!("portageq: {e}");
                return EX_INTERNAL;
            }
            match dep_expand(&root, raw) {
                Ok(a) => (a, None),
                Err(ExpandError::Invalid) => {
                    eprintln!("ERROR: Invalid atom: '{raw}'");
                    // Real: has_version catches InvalidAtom (exit 2);
                    // best_version dies with a traceback (exit 1). We keep
                    // the exit code and print the line instead of the
                    // traceback.
                    return match cmd {
                        Cmd::HasVersion => 2,
                        Cmd::BestVersion => 1,
                    };
                }
                Err(ExpandError::Ambiguous(list)) => {
                    // Real: AmbiguousPackageName, uncaught, exit 1.
                    eprintln!("ERROR: ambiguous package name '{raw}': {}", list.join(" "));
                    return 1;
                }
            }
        }
    };
    if let Err(e) = setup_backend(&root) {
        eprintln!("portageq: {e}");
        return EX_INTERNAL;
    }

    // Rule 7: the resolver's own installed-package matcher.
    let implicit = || -> HashSet<String> {
        crate::pretend::load_repos_and_config(&portage_repo::config_root_from_env(), &root)
            .map(|(_, config)| config.iuse_effective)
            .unwrap_or_default()
    };
    let best = portage_repo::best_installed_match(&root, &atom, unevaluated.as_deref(), &implicit);
    match cmd {
        // Rule 8.
        Cmd::HasVersion => u8::from(best.is_none()),
        // Rule 9.
        Cmd::BestVersion => {
            let line = match (&best, portage_dep::parse_atom(&atom)) {
                (Some(version), Some(a)) => format!("{}/{}-{version}", a.category, a.package),
                _ => String::new(),
            };
            println!("{line}");
            0
        }
    }
}

// ---------------------------------------------------------------------
// Backend selection
// ---------------------------------------------------------------------

/// Register the database `mrg` exported for this root, if any. The
/// default (files) needs nothing: `portage_vdb::for_root` falls back to it.
fn setup_backend(eroot: &Path) -> Result<(), String> {
    let backend = std::env::var("PORTUALE_VDB_BACKEND")
        .ok()
        .filter(|v| !v.is_empty());
    let Some(backend) = backend else {
        return Ok(());
    };
    let kind: BackendKind = backend.parse().map_err(|_| {
        format!("PORTUALE_VDB_BACKEND={backend:?} is not one of files, sqlite, redb")
    })?;
    if kind == BackendKind::Files {
        return Ok(());
    }
    // Which root does the database belong to?
    if let Some(db_root) = std::env::var_os("PORTUALE_VDB_ROOT").filter(|v| !v.is_empty())
        && !same_root(Path::new(&db_root), eroot)
    {
        return Ok(());
    }
    let path = std::env::var_os("PORTUALE_VDB_PATH")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("PORTUALE_VDB_BACKEND={backend} needs PORTUALE_VDB_PATH"))?;
    let db = open_readonly(kind, &path)?;
    portage_vdb::register(eroot, db);
    Ok(())
}

type SharedDb = std::sync::Arc<dyn portage_vdb::InstalledDb>;

fn open_readonly(kind: BackendKind, path: &Path) -> Result<SharedDb, String> {
    match kind {
        BackendKind::Sqlite => open_sqlite_readonly(path),
        BackendKind::Redb => open_redb_readonly(path),
        BackendKind::Files => Err("the files backend has no database file".into()),
    }
}

#[cfg(feature = "vdb-sqlite")]
fn open_sqlite_readonly(path: &Path) -> Result<SharedDb, String> {
    portage_vdb::SqliteDb::open_readonly(path)
        .map(|db| std::sync::Arc::new(db) as SharedDb)
        .map_err(|e| format!("cannot open the sqlite VDB {}: {e}", path.display()))
}

#[cfg(not(feature = "vdb-sqlite"))]
fn open_sqlite_readonly(_path: &Path) -> Result<SharedDb, String> {
    Err(
        "the sqlite VDB backend is not available: this portuale was built without the \
         vdb-sqlite feature"
            .into(),
    )
}

/// redb allows one process per file. A read-only open succeeds only when
/// nobody holds the file read-write (a helper run by hand, outside `mrg`).
/// Under a running `mrg` on redb the file is held, so the open is `Busy`
/// and the query fails with exit 4 until the parent pipe exists (plan S6.3,
/// `PORTUALE_VDB_IPC`).
#[cfg(feature = "vdb-redb")]
fn open_redb_readonly(path: &Path) -> Result<SharedDb, String> {
    match portage_vdb::RedbDb::open_readonly(path) {
        Ok(db) => Ok(std::sync::Arc::new(db) as SharedDb),
        Err(e @ portage_vdb::Error::Busy { .. }) => Err(format!(
            "cannot open the redb VDB {}: it is held open by another process (the mrg that \
             started this helper): {e}; querying a redb VDB held by mrg needs the parent pipe, \
             which arrives in plan step S6.3",
            path.display()
        )),
        Err(e) => Err(format!("cannot open the redb VDB {}: {e}", path.display())),
    }
}

#[cfg(not(feature = "vdb-redb"))]
fn open_redb_readonly(_path: &Path) -> Result<SharedDb, String> {
    Err(
        "the redb VDB backend is not available: this portuale was built without the vdb-redb \
         feature"
            .into(),
    )
}

/// Lexical normalisation (`.` and repeated `/` dropped, trailing `/`
/// ignored, `..` not resolved), then canonical comparison when both exist.
fn same_root(a: &Path, b: &Path) -> bool {
    fn lex(p: &Path) -> Vec<std::path::Component<'_>> {
        p.components()
            .filter(|c| !matches!(c, std::path::Component::CurDir))
            .collect()
    }
    if lex(a) == lex(b) {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

// ---------------------------------------------------------------------
// EAPI-aware atom validation (real `Atom(..., eapi=)`)
// ---------------------------------------------------------------------

/// Numeric EAPI for the attribute comparisons, `None` for a missing or
/// unsupported EAPI: real `_get_eapi_attrs` then returns the permissive
/// attribute set (`eapi.py:48`).
fn eapi_number(eapi: Option<&str>) -> Option<u32> {
    let eapi = eapi?;
    if !portage_repo::md5_dict::eapi_is_supported(eapi) {
        return None;
    }
    let digits: String = eapi
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Real `eapi_has_repo_deps`: false for every numbered EAPI, true for the
/// permissive (missing or unsupported) attribute set. So under
/// `EBUILD_PHASE` with a real `EAPI`, `::repo` atoms are invalid.
fn eapi_has_repo_deps(eapi: Option<&str>) -> bool {
    eapi_number(eapi).is_none()
}

/// The error real `Atom(raw, allow_repo=..., eapi=eapi)` raises for an
/// atom the EAPI-less parse accepted, if any (`dep/__init__.py:2090-2150`).
fn eapi_violation(atom: &portage_dep::Atom, raw: &str, eapi: Option<&str>) -> Option<String> {
    let n = eapi_number(eapi)?;
    let eapi = eapi?;
    // Pre-EAPI-5 slot grammar has no sub-slot and no operator: the slot
    // regex rejects the atom outright (`InvalidAtom(self._string)`).
    if n < 5 && (atom.slot_operator.is_some() || atom.sub_slot.is_some()) {
        return Some(raw.to_string());
    }
    if atom.slot.is_some() && n < 1 {
        return Some(format!("Slot deps are not allowed in EAPI {eapi}: '{raw}'"));
    }
    if let Some(deps) = atom.use_deps.as_ref().filter(|d| !d.is_empty()) {
        if n < 2 {
            return Some(format!("Use deps are not allowed in EAPI {eapi}: '{raw}'"));
        }
        if n < 4 && deps.iter().any(|d| d.default.is_some()) {
            return Some(format!(
                "Use dep defaults are not allowed in EAPI {eapi}: '{raw}'"
            ));
        }
    }
    if atom.blocker == portage_dep::Blocker::Strong && n < 2 {
        return Some(format!(
            "Strong blocks are not allowed in EAPI {eapi}: '{raw}'"
        ));
    }
    None
}

// ---------------------------------------------------------------------
// QA notices (real `elog("eqawarn", lines)`, portageq:1408-1425)
// ---------------------------------------------------------------------

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Real portageq runs `bash -c "source '$PORTAGE_BIN_PATH/isolated-functions.sh' ;
/// eqawarn '<line>' ; ..."`, so the notice goes through the ebuild's own
/// elog machinery (log file, `PORTAGE_ELOG_*`). Do the same when
/// `PORTAGE_BIN_PATH` names a directory holding `isolated-functions.sh`.
/// Otherwise (no phase environment) print ` * <line>` on stderr, the
/// plain-text form of `eqawarn`'s output.
fn eqawarn(lines: &[String]) {
    if let Some(bin) = std::env::var_os("PORTAGE_BIN_PATH").filter(|v| !v.is_empty()) {
        let bin = PathBuf::from(bin);
        let script = bin.join("isolated-functions.sh");
        if script.is_file() {
            let mut cmd = format!("source {} ; ", shell_quote(&script.to_string_lossy()));
            for line in lines {
                cmd.push_str(&format!("eqawarn {} ; ", shell_quote(line)));
            }
            if std::process::Command::new("bash")
                .arg("-c")
                .arg(cmd)
                .status()
                .is_ok()
            {
                return;
            }
        }
    }
    for line in lines {
        eprintln!(" * {line}");
    }
}

// ---------------------------------------------------------------------
// dep_expand (real `portage/dbapi/dep_expand.py`, the fall-through path)
// ---------------------------------------------------------------------

enum ExpandError {
    Invalid,
    Ambiguous(Vec<String>),
}

/// What `vardb.match()` does to a string the strict `Atom()` parse
/// rejected: a missing `=` prefix is allowed (`cat/pkg-1.0`), and a name
/// without a category is expanded to the one category that has it
/// installed (`cpv_expand`); none installed leaves `null/<pn>`, which
/// matches nothing.
fn dep_expand(root: &Path, raw: &str) -> Result<String, ExpandError> {
    let orig = raw.strip_prefix('*').unwrap_or(raw);
    if orig.is_empty() {
        return Err(ExpandError::Invalid);
    }
    let has_cat = orig.split(':').next().is_some_and(|s| s.contains('/'));
    let mut candidate = orig.to_string();
    if !has_cat && let Some(i) = orig.find(|c: char| c.is_alphanumeric() || c == '_') {
        candidate = format!("{}null/{}", &orig[..i], &orig[i..]);
    }
    let mut orig_dep = orig.to_string();
    let atom = match portage_dep::parse_atom(&candidate) {
        Some(a) => a,
        None => {
            let with_eq = format!("={candidate}");
            let a = portage_dep::parse_atom(&with_eq).ok_or(ExpandError::Invalid)?;
            candidate = with_eq;
            orig_dep = format!("={orig_dep}");
            a
        }
    };
    if has_cat {
        return Ok(candidate);
    }
    let pn = atom.package;
    let db = portage_vdb::for_root(root);
    let cats: Vec<String> = db
        .categories()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| !portage_repo::installed_candidates(root, c, &pn).is_empty())
        .collect();
    match cats.as_slice() {
        [] => Ok(orig_dep.replacen(&pn, &format!("null/{pn}"), 1)),
        [one] => Ok(orig_dep.replacen(&pn, &format!("{one}/{pn}"), 1)),
        many => Err(ExpandError::Ambiguous(
            many.iter().map(|c| format!("{c}/{pn}")).collect(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Output};
    #[cfg(feature = "vdb-sqlite")]
    use std::sync::OnceLock;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    fn binary() -> PathBuf {
        let mut bin = std::env::current_exe().unwrap();
        bin.pop();
        if bin.ends_with("deps") {
            bin.pop();
        }
        bin.push("portuale");
        bin
    }

    /// A sqlite conversion of the fixture VDB (`copy_all`), made once.
    #[cfg(feature = "vdb-sqlite")]
    fn sqlite_db() -> &'static Path {
        static DB: OnceLock<PathBuf> = OnceLock::new();
        DB.get_or_init(|| {
            let tmp = portage_util::TempDir::new("portageq_sqlite").keep();
            let path = tmp.join("vdb.sqlite");
            let src = portage_vdb::FilesDb::new(&fixtures());
            let dst = portage_vdb::SqliteDb::open(&path).unwrap();
            portage_vdb::copy_all(&src, &dst, false).unwrap();
            path
        })
    }

    #[derive(Clone, Copy)]
    enum Backend {
        Files,
        #[cfg(feature = "vdb-sqlite")]
        Sqlite,
    }

    fn portageq(backend: Backend, args: &[&str], env: &[(&str, &str)]) -> Output {
        let fx = fixtures();
        let mut c = Command::new(binary());
        c.arg("portageq")
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("PORTAGE_CONFIGROOT", &fx)
            .env("ROOT", &fx);
        match backend {
            Backend::Files => {}
            #[cfg(feature = "vdb-sqlite")]
            Backend::Sqlite => {
                c.env("PORTUALE_VDB_BACKEND", "sqlite")
                    .env("PORTUALE_VDB_PATH", sqlite_db())
                    .env("PORTUALE_VDB_ROOT", &fx);
            }
        }
        for (k, v) in env {
            c.env(k, v);
        }
        c.output().expect("portuale portageq spawns")
    }

    /// `(rc, stdout, stderr)`.
    type Res = (i32, String, String);

    fn res(o: &Output) -> Res {
        (
            o.status.code().unwrap(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        )
    }

    /// Run `<cmd> <fixtures> <atom>` on the files tree and, with the
    /// sqlite feature, on the sqlite conversion of the same root; the two
    /// must be identical (S6.2 acceptance). Returns the common result.
    fn q(cmd: &str, atom: &str, env: &[(&str, &str)]) -> Res {
        let fx = fixtures();
        let fx = fx.to_str().unwrap();
        let files = res(&portageq(Backend::Files, &[cmd, fx, atom], env));
        #[cfg(feature = "vdb-sqlite")]
        {
            let sqlite = res(&portageq(Backend::Sqlite, &[cmd, fx, atom], env));
            assert_eq!(
                files, sqlite,
                "{cmd} {atom} {env:?}: files vs sqlite differ"
            );
        }
        files
    }

    fn has(atom: &str, env: &[(&str, &str)]) -> i32 {
        let (rc, out, _) = q("has_version", atom, env);
        assert_eq!(out, "", "has_version prints nothing (rule 8)");
        rc
    }

    fn best(atom: &str, env: &[(&str, &str)]) -> String {
        let (rc, out, _) = q("best_version", atom, env);
        assert_eq!(rc, 0, "best_version {atom} exits 0 (rule 9)");
        out
    }

    /// Evidence captures 1, 2; rule 8: exit 0 when installed, 1 when not,
    /// nothing printed.
    #[test]
    fn found_and_not_found() {
        assert_eq!(has("dev-libs/keeper", &[]), 0);
        assert_eq!(has("dev-libs/nonexistent", &[]), 1);
    }

    /// Evidence capture 3; rule 7 (version operators).
    #[test]
    fn versioned_atoms() {
        assert_eq!(has(">=dev-libs/dualslotpkg-1.5", &[]), 0);
        assert_eq!(has(">dev-libs/dualslotpkg-2.0", &[]), 1);
        assert_eq!(has("<dev-libs/dualslotpkg-1", &[]), 1);
        assert_eq!(has("=dev-libs/unmergepkg-1.0", &[]), 0);
        assert_eq!(has("=dev-libs/unmergepkg-3.0", &[]), 1);
        assert_eq!(has("~dev-libs/unmergepkg-2.0", &[]), 0);
        assert_eq!(has("=dev-libs/unmergepkg-1*", &[]), 0);
    }

    /// Evidence capture 4; rule 7 (slot and sub-slot).
    #[test]
    fn slot_atoms() {
        assert_eq!(has("dev-libs/dualslotpkg:1", &[]), 0);
        assert_eq!(has("dev-libs/dualslotpkg:3", &[]), 1);
        assert_eq!(has("dev-libs/r25lib:0/1", &[]), 0);
        assert_eq!(has("dev-libs/r25lib:0/2", &[]), 1);
        assert_eq!(
            best("dev-libs/dualslotpkg:1", &[]),
            "dev-libs/dualslotpkg-1.0\n"
        );
        assert_eq!(
            best("dev-libs/dualslotpkg:2", &[]),
            "dev-libs/dualslotpkg-2.0\n"
        );
    }

    /// Evidence capture 14; rules 7 and 8 (`::repo`, allowed outside a
    /// phase).
    #[test]
    fn repo_atoms() {
        assert_eq!(has("dev-libs/newrepopkg::oldrepo", &[]), 0);
        assert_eq!(has("dev-libs/newrepopkg::testrepo", &[]), 1);
        assert_eq!(has("dev-libs/keeper::testrepo", &[]), 0);
    }

    /// Evidence captures 5, 6; rule 7: `[flag]` / `[-flag]` against the
    /// installed package's own `USE`, and a flag outside its `IUSE` never
    /// matches (real `_match_use`).
    #[test]
    fn use_deps_against_installed_use() {
        // infoinstpkg: USE=alpha, IUSE=alpha beta.
        assert_eq!(has("dev-libs/infoinstpkg[alpha]", &[]), 0);
        assert_eq!(has("dev-libs/infoinstpkg[beta]", &[]), 1);
        assert_eq!(has("dev-libs/infoinstpkg[-alpha]", &[]), 1);
        assert_eq!(has("dev-libs/infoinstpkg[-beta]", &[]), 0);
        assert_eq!(has("dev-libs/infoinstpkg[alpha,-beta]", &[]), 0);
        assert_eq!(has("dev-libs/infoinstpkg[nosuchflag]", &[]), 1);
        assert_eq!(has("dev-libs/infoinstpkg[nosuchflag(+)]", &[]), 0);
        assert_eq!(has("dev-libs/infoinstpkg[nosuchflag(-)]", &[]), 1);
    }

    /// Evidence captures 7, 8; rule 2: `[flag?]` is evaluated against
    /// `$USE` only when `USE` is set; the dropped conditional's flag must
    /// still be in the installed `IUSE` (real checks the unevaluated
    /// atom's `.required`).
    #[test]
    fn use_conditionals_follow_the_environment_use() {
        // beta is in IUSE but not enabled.
        assert_eq!(has("dev-libs/infoinstpkg[beta?]", &[("USE", "beta")]), 1);
        assert_eq!(has("dev-libs/infoinstpkg[beta?]", &[("USE", "")]), 0);
        assert_eq!(has("dev-libs/infoinstpkg[beta?]", &[]), 0);
        assert_eq!(has("dev-libs/infoinstpkg[alpha?]", &[("USE", "alpha")]), 0);
        assert_eq!(has("dev-libs/infoinstpkg[!alpha?]", &[("USE", "alpha")]), 0);
        assert_eq!(has("dev-libs/infoinstpkg[!beta?]", &[("USE", "")]), 0);
        assert_eq!(has("dev-libs/infoinstpkg[!alpha?]", &[("USE", "")]), 1);
        assert_eq!(has("dev-libs/infoinstpkg[alpha=]", &[("USE", "alpha")]), 0);
        assert_eq!(has("dev-libs/infoinstpkg[alpha=]", &[("USE", "")]), 1);
        assert_eq!(has("dev-libs/infoinstpkg[beta=]", &[("USE", "")]), 0);
        // Not in the installed IUSE: never matches, set or not.
        assert_eq!(has("dev-libs/infoinstpkg[nosuchflag?]", &[("USE", "")]), 1);
        assert_eq!(has("dev-libs/infoinstpkg[nosuchflag?]", &[]), 1);
        // best_version evaluates the same way.
        assert_eq!(
            best("dev-libs/infoinstpkg[beta?]", &[("USE", "beta")]),
            "\n"
        );
        assert_eq!(
            best("dev-libs/infoinstpkg[beta?]", &[("USE", "")]),
            "dev-libs/infoinstpkg-1.0\n"
        );
    }

    /// Evidence capture 10; rule 4: strict (`EBUILD_PHASE` set) and
    /// unparsable: stderr line, exit 2, nothing on stdout, no QA notice.
    #[test]
    fn strict_invalid_atom_exits_2() {
        let env = [("EBUILD_PHASE", "setup"), ("EAPI", "8")];
        for cmd in ["has_version", "best_version"] {
            let (rc, out, err) = q(cmd, "garbage-atom", &env);
            assert_eq!(rc, 2, "{cmd}");
            assert_eq!(out, "");
            assert_eq!(err, "ERROR: Invalid atom: 'garbage-atom'\n");
        }
        // Rule 3: with a real EAPI, `::repo` is not allowed under strict.
        let (rc, _, err) = q("has_version", "dev-libs/keeper::testrepo", &env);
        assert_eq!(rc, 2);
        assert_eq!(err, "ERROR: Invalid atom: 'dev-libs/keeper::testrepo'\n");
        // ... but is when EAPI is unset (permissive attributes).
        assert_eq!(
            has("dev-libs/keeper::testrepo", &[("EBUILD_PHASE", "setup")]),
            0
        );
    }

    /// Rules 3 and 5: strict, the atom parses without an EAPI but is
    /// invalid for `EAPI`: a QA notice, and the atom still matches. With
    /// no `PORTAGE_BIN_PATH` the notice is ` * QA Notice: ...` on stderr.
    #[test]
    fn strict_eapi_only_invalid_warns_and_still_matches() {
        let env = [("EBUILD_PHASE", "setup"), ("EAPI", "1")];
        let (rc, out, err) = q("has_version", "dev-libs/infoinstpkg[alpha]", &env);
        assert_eq!((rc, out.as_str()), (0, ""));
        assert_eq!(
            err,
            " * QA Notice: has_version: Use deps are not allowed in EAPI 1: \
             'dev-libs/infoinstpkg[alpha]'\n"
        );
        let (rc, out, err) = q("best_version", "dev-libs/infoinstpkg[alpha]", &env);
        assert_eq!((rc, out.as_str()), (0, "dev-libs/infoinstpkg-1.0\n"));
        assert!(err.contains("QA Notice: best_version: Use deps are not allowed in EAPI 1"));
        // A valid atom for the EAPI: no notice.
        let (_, _, err) = q(
            "has_version",
            "dev-libs/infoinstpkg[alpha]",
            &[("EBUILD_PHASE", "setup"), ("EAPI", "8")],
        );
        assert_eq!(err, "");
        // Not strict: EAPI is ignored.
        let (_, _, err) = q(
            "has_version",
            "dev-libs/infoinstpkg[alpha]",
            &[("EAPI", "1")],
        );
        assert_eq!(err, "");
        // Other EAPI rules.
        let a = |s: &str| portage_dep::parse_atom(s).unwrap();
        let v = |s: &str, e: &str| eapi_violation(&a(s), s, Some(e));
        assert_eq!(
            v("dev-libs/x:1", "0").unwrap(),
            "Slot deps are not allowed in EAPI 0: 'dev-libs/x:1'"
        );
        assert!(v("dev-libs/x:1", "1").is_none());
        assert_eq!(v("dev-libs/x:1=", "4").unwrap(), "dev-libs/x:1=");
        assert!(v("dev-libs/x:1=", "5").is_none());
        assert!(v("dev-libs/x[a(+)]", "3").unwrap().contains("defaults"));
        assert!(v("dev-libs/x[a(+)]", "4").is_none());
        assert!(v("!!dev-libs/x", "1").unwrap().contains("Strong blocks"));
        assert!(v("!!dev-libs/x", "2").is_none());
        assert!(eapi_violation(&a("dev-libs/x[a]"), "dev-libs/x[a]", None).is_none());
        assert!(v("dev-libs/x[a]", "no-such-eapi").is_none());
    }

    /// The QA notice goes through `eqawarn` from
    /// `$PORTAGE_BIN_PATH/isolated-functions.sh` when that file exists
    /// (real `elog("eqawarn", ...)`).
    #[test]
    fn qa_notice_goes_through_isolated_functions_eqawarn() {
        let tmp = portage_util::TempDir::new("portageq_eqawarn").keep();
        std::fs::write(
            tmp.join("isolated-functions.sh"),
            "eqawarn() { echo \"EQAWARN[$*]\" >&2; }\n",
        )
        .unwrap();
        let bin = tmp.to_str().unwrap();
        let (rc, _, err) = q(
            "has_version",
            "dev-libs/infoinstpkg[alpha]",
            &[
                ("EBUILD_PHASE", "setup"),
                ("EAPI", "1"),
                ("PORTAGE_BIN_PATH", bin),
            ],
        );
        assert_eq!(rc, 0);
        assert_eq!(
            err,
            "EQAWARN[QA Notice: has_version: Use deps are not allowed in EAPI 1: \
             'dev-libs/infoinstpkg[alpha]']\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Evidence capture 9; rule 6: not strict, a bare unknown name is
    /// expanded as a package name and does not match (exit 1, silent). A
    /// bare name that is installed expands to its category (real
    /// `dep_expand` / `cpv_expand`), and a missing `=` is tolerated.
    #[test]
    fn non_strict_bare_names_expand() {
        let (rc, out, err) = q("has_version", "garbage-atom", &[]);
        assert_eq!((rc, out.as_str(), err.as_str()), (1, "", ""));
        assert_eq!(has("keeper", &[]), 0);
        assert_eq!(best("keeper", &[]), "dev-libs/keeper-1.0\n");
        assert_eq!(best("unmergepkg", &[]), "dev-libs/unmergepkg-2.0\n");
        assert_eq!(has("dev-libs/unmergepkg-1.0", &[]), 0);
        assert_eq!(has("dev-libs/unmergepkg-3.0", &[]), 1);
        assert_eq!(best("virtual/libc-1.0", &[]), "virtual/libc-1.0\n");
    }

    /// Rule 6: not strict, a malformed atom: `has_version` prints
    /// `ERROR: Invalid atom` and exits 2; `best_version` exits 1 (real
    /// dies with a traceback; documented divergence: the same ERROR line,
    /// no traceback).
    #[test]
    fn non_strict_malformed_atom() {
        for atom in ["dev-libs/keeper[", "=dev-libs/keeper", "dev-libs/keeper:"] {
            let (rc, out, err) = q("has_version", atom, &[]);
            assert_eq!(rc, 2, "has_version {atom}");
            assert_eq!(out, "");
            assert_eq!(err, format!("ERROR: Invalid atom: '{atom}'\n"));
            let (rc, out, err) = q("best_version", atom, &[]);
            assert_eq!(rc, 1, "best_version {atom}");
            assert_eq!(out, "");
            assert_eq!(err, format!("ERROR: Invalid atom: '{atom}'\n"));
        }
    }

    /// Evidence captures 11, 13; rule 9: `best_version` prints the highest
    /// matching `cat/pf`.
    #[test]
    fn best_version_prints_the_highest_match() {
        assert_eq!(
            best("dev-libs/unmergepkg", &[]),
            "dev-libs/unmergepkg-2.0\n"
        );
        assert_eq!(
            best("dev-libs/dualslotpkg", &[]),
            "dev-libs/dualslotpkg-2.0\n"
        );
        assert_eq!(
            best("<dev-libs/unmergepkg-2", &[]),
            "dev-libs/unmergepkg-1.0\n"
        );
        assert_eq!(best("dev-libs/blk0y", &[]), "dev-libs/blk0y-3\n");
    }

    /// Evidence capture 12; rule 9: nothing matches: an empty line, exit 0.
    #[test]
    fn best_version_no_match_prints_an_empty_line() {
        let (rc, out, err) = q("best_version", "dev-libs/nonexistent", &[]);
        assert_eq!((rc, out.as_str(), err.as_str()), (0, "\n", ""));
    }

    /// Evidence capture 15; rules 1 and 10: fewer than two arguments:
    /// `ERROR: insufficient parameters!` on stdout, exit 3.
    #[test]
    fn insufficient_parameters() {
        let fx = fixtures();
        for cmd in ["has_version", "best_version"] {
            for args in [vec![cmd], vec![cmd, fx.to_str().unwrap()]] {
                let (rc, out, err) = res(&portageq(Backend::Files, &args, &[]));
                assert_eq!(rc, 3, "{args:?}");
                assert_eq!(out, "ERROR: insufficient parameters!\n");
                assert_eq!(err, "");
            }
        }
    }

    /// Other portageq commands are refused by name, not emulated; a bad
    /// <eroot> is real's `Not a directory` usage error (exit 64).
    #[test]
    fn unknown_command_and_bad_eroot() {
        let (rc, out, err) = res(&portageq(Backend::Files, &["envvar", "/", "ROOT"], &[]));
        assert_eq!(rc, 1);
        assert_eq!(out, "");
        assert_eq!(err, "not implemented by portuale portageq: envvar\n");
        let (rc, out, err) = res(&portageq(
            Backend::Files,
            &["has_version", "/no/such/dir", "dev-libs/keeper"],
            &[],
        ));
        assert_eq!(rc, 64);
        assert_eq!(out, "");
        assert!(
            err.starts_with("Not a directory: '/no/such/dir'\n"),
            "{err}"
        );
    }

    /// `argv[0] == "portageq"` runs the applet (a symlink beside the
    /// binary, like `emerge` / `ebuild`).
    #[cfg(unix)]
    #[test]
    fn argv0_dispatch() {
        let tmp = portage_util::TempDir::new("portageq_argv0").keep();
        let link = tmp.join("portageq");
        std::os::unix::fs::symlink(binary(), &link).unwrap();
        let fx = fixtures();
        let o = Command::new(&link)
            .args(["best_version", fx.to_str().unwrap(), "dev-libs/keeper"])
            .env("ROOT", &fx)
            .output()
            .unwrap();
        assert_eq!(
            res(&o),
            (0, "dev-libs/keeper-1.0\n".to_string(), String::new())
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The database belongs to one root (`PORTUALE_VDB_ROOT`): for any
    /// other <eroot> (`has_version -b`: BROOT `/` while ROOT is a chroot)
    /// the files layout is used. An empty database for the matching root
    /// answers "nothing installed", which proves it is the one opened.
    #[cfg(feature = "vdb-sqlite")]
    #[test]
    fn database_is_used_only_for_its_own_root() {
        let tmp = portage_util::TempDir::new("portageq_root").keep();
        let empty = tmp.join("empty.sqlite");
        portage_vdb::SqliteDb::open(&empty).unwrap();
        let fx = fixtures();
        let fxs = fx.to_str().unwrap();
        let run = |db_root: Option<&str>| {
            let mut env = vec![
                ("PORTUALE_VDB_BACKEND", "sqlite"),
                ("PORTUALE_VDB_PATH", empty.to_str().unwrap()),
            ];
            if let Some(r) = db_root {
                env.push(("PORTUALE_VDB_ROOT", r));
            }
            res(&portageq(
                Backend::Files,
                &["has_version", fxs, "dev-libs/keeper"],
                &env,
            ))
            .0
        };
        // Same root (also spelled with a trailing slash): the empty db.
        assert_eq!(run(Some(fxs)), 1);
        assert_eq!(run(Some(&format!("{fxs}/"))), 1);
        // Another root: the files tree of <eroot>.
        assert_eq!(run(Some("/some/other/root")), 0);
        // No root named: the database applies to the given <eroot>.
        assert_eq!(run(None), 1);
        // A missing or unusable database is an internal error, not "not
        // installed".
        let (rc, _, err) = res(&portageq(
            Backend::Files,
            &["has_version", fxs, "dev-libs/keeper"],
            &[
                ("PORTUALE_VDB_BACKEND", "sqlite"),
                (
                    "PORTUALE_VDB_PATH",
                    tmp.join("missing.sqlite").to_str().unwrap(),
                ),
            ],
        ));
        assert_eq!(rc, 4);
        assert!(err.contains("cannot open the sqlite VDB"), "{err}");
        let (rc, _, err) = res(&portageq(
            Backend::Files,
            &["has_version", fxs, "dev-libs/keeper"],
            &[
                ("PORTUALE_VDB_BACKEND", "redb"),
                ("PORTUALE_VDB_PATH", "/x"),
            ],
        ));
        assert_eq!(rc, 4);
        assert!(err.contains("redb"), "{err}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// S5.4: a redb database nobody holds is read directly (a helper run
    /// outside `mrg`); one held by another process (the parent `mrg`) is a
    /// clean exit 4 that points at the parent pipe of plan step S6.3.
    #[cfg(feature = "vdb-redb")]
    #[test]
    fn redb_is_read_when_free_and_exit_4_when_held() {
        let tmp = portage_util::TempDir::new("portageq_redb").keep();
        let path = tmp.join("vdb.redb");
        let fx = fixtures();
        let fxs = fx.to_str().unwrap();
        {
            let src = portage_vdb::FilesDb::new(&fx);
            let dst = portage_vdb::RedbDb::open(&path).unwrap();
            portage_vdb::copy_all(&src, &dst, false).unwrap();
        }
        let env = [
            ("PORTUALE_VDB_BACKEND", "redb"),
            ("PORTUALE_VDB_PATH", path.to_str().unwrap()),
            ("PORTUALE_VDB_ROOT", fxs),
        ];
        let ask = |atom: &str| res(&portageq(Backend::Files, &["has_version", fxs, atom], &env));
        let files = res(&portageq(
            Backend::Files,
            &["has_version", fxs, "dev-libs/keeper"],
            &[],
        ));
        // Free: the same answer as the files layout, for a hit and a miss.
        assert_eq!(ask("dev-libs/keeper"), files);
        assert_eq!(ask("dev-libs/not-installed-at-all").0, 1);
        // Held (read-write, like mrg): exit 4 with the S6.3 message.
        let held = portage_vdb::RedbDb::open(&path).unwrap();
        let (rc, _, err) = ask("dev-libs/keeper");
        assert_eq!(rc, 4, "{err}");
        assert!(
            err.contains("already open") && err.contains("S6.3"),
            "{err}"
        );
        drop(held);
        assert_eq!(ask("dev-libs/keeper"), files);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn same_root_compares_spellings() {
        assert!(same_root(Path::new("/a/b"), Path::new("/a/b/")));
        assert!(same_root(Path::new("/a/./b"), Path::new("/a/b")));
        assert!(!same_root(Path::new("/a/b"), Path::new("/a/c")));
        assert!(same_root(Path::new("/"), Path::new("//")));
    }

    /// Optional: the same queries against the real `bin/portageq`
    /// (`PORTUALE_REAL_PORTAGEQ=1`; needs python3 and the
    /// `3rdparty/portage` checkout). Compares exit code and stdout.
    #[test]
    fn matches_real_portageq_on_the_fixture_root() {
        if std::env::var_os("PORTUALE_REAL_PORTAGEQ").as_deref() != Some("1".as_ref()) {
            eprintln!("skipped: set PORTUALE_REAL_PORTAGEQ=1 to compare with real portageq");
            return;
        }
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../3rdparty/portage");
        let fx = fixtures().canonicalize().unwrap();
        let fxs = fx.to_str().unwrap();
        type Case<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);
        let cases: &[Case] = &[
            ("has_version", "dev-libs/keeper", &[]),
            ("has_version", "dev-libs/nonexistent", &[]),
            ("has_version", ">=dev-libs/dualslotpkg-1.5", &[]),
            ("has_version", "dev-libs/dualslotpkg:3", &[]),
            ("has_version", "dev-libs/r25lib:0/1", &[]),
            ("has_version", "dev-libs/newrepopkg::oldrepo", &[]),
            ("has_version", "dev-libs/newrepopkg::testrepo", &[]),
            ("has_version", "dev-libs/infoinstpkg[alpha]", &[]),
            ("has_version", "dev-libs/infoinstpkg[-alpha]", &[]),
            (
                "has_version",
                "dev-libs/infoinstpkg[beta?]",
                &[("USE", "beta")],
            ),
            ("has_version", "dev-libs/infoinstpkg[beta?]", &[("USE", "")]),
            (
                "has_version",
                "dev-libs/infoinstpkg[nosuchflag?]",
                &[("USE", "")],
            ),
            (
                "has_version",
                "dev-libs/infoinstpkg[alpha=]",
                &[("USE", "")],
            ),
            ("has_version", "garbage-atom", &[]),
            ("has_version", "keeper", &[]),
            ("has_version", "dev-libs/unmergepkg-1.0", &[]),
            ("has_version", "dev-libs/keeper[", &[]),
            ("best_version", "dev-libs/unmergepkg", &[]),
            ("best_version", "dev-libs/dualslotpkg:1", &[]),
            ("best_version", "dev-libs/nonexistent", &[]),
            ("best_version", "keeper", &[]),
            (
                "has_version",
                "garbage-atom",
                &[("EBUILD_PHASE", "setup"), ("EAPI", "8")],
            ),
            (
                "has_version",
                "dev-libs/keeper::testrepo",
                &[("EBUILD_PHASE", "setup"), ("EAPI", "8")],
            ),
        ];
        for (cmd, atom, env) in cases {
            let mut c = Command::new("python3");
            c.arg(checkout.join("bin/portageq"))
                .args([cmd, fxs, atom])
                .env("PYTHONPATH", checkout.join("lib"))
                .env("PORTAGE_CONFIGROOT", &fx)
                .env("ROOT", &fx)
                .env_remove("USE")
                .env_remove("EBUILD_PHASE")
                .env_remove("EAPI");
            for (k, v) in *env {
                c.env(k, v);
            }
            if env.iter().any(|(k, _)| *k == "EBUILD_PHASE") {
                c.env("PORTAGE_BIN_PATH", checkout.join("bin"));
            }
            let real = c.output().expect("python3 portageq spawns");
            let ours = portageq(Backend::Files, &[cmd, fxs, atom], env);
            assert_eq!(
                real.status.code(),
                ours.status.code(),
                "{cmd} {atom} {env:?}: exit code"
            );
            assert_eq!(
                String::from_utf8_lossy(&real.stdout),
                String::from_utf8_lossy(&ours.stdout),
                "{cmd} {atom} {env:?}: stdout"
            );
        }
    }
}
