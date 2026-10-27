"""R4 census: run Portage (through sc-trace.py) and portuale with the same argv/env on a
fresh scratch copy of pmtest/fixtures per run. Jobs come from a JSON list
[{id, args, env}]; results go to <out>/<id>.{portage,portuale}.{stdout,stderr,rc} and
<id>.trace.jsonl.  Usage: python3 -I census.py <jobs.json> <outdir> [workers]"""
import json, os, re, shutil, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
SRC = "/home/vivo/repo/PORTUALE/pmtest/fixtures"
PORTUALE = "/home/vivo/repo/PORTUALE/portuale/target/release/portuale"
HERE = os.path.dirname(os.path.abspath(__file__))
TRACE = os.path.join(HERE, "emerge-trace")
FILTER = os.path.join(HERE, "ns-run.sh")
FILTER_SRC = os.path.join(HERE, "repos-filter-global.conf")
WORK = "/var/tmp/claude-r4/j"
STRIP = ("USE ACCEPT_KEYWORDS USE_EXPAND USE_EXPAND_UNPREFIXED USE_EXPAND_IMPLICIT USE_EXPAND_HIDDEN "
         "IUSE_IMPLICIT ACCEPT_LICENSE ACCEPT_PROPERTIES ACCEPT_RESTRICT PKGDIR PORTAGE_LOGDIR PORTAGE_BINHOST "
         "FEATURES CHOST CBUILD CTARGET MAKEOPTS EMERGE_DEFAULT_OPTS GENTOO_MIRRORS VIDEO_CARDS "
         "PYTHON_TARGETS PYTHON_SINGLE_TARGET LINGUAS L10N CPU_FLAGS_X86 ELIBC KERNEL USERLAND ABI_X86 "
         "BINPKG_FORMAT CONFIG_PROTECT CONFIG_PROTECT_MASK ENV_UNSET PORTAGE_REPOSITORIES").split()

def repos(fx):
    out = []
    for line in open(os.path.join(fx, "etc/portage/repos.conf/repos.conf")):
        m = re.match(r"^(location\s*=\s*)(\S.*)$", line.rstrip("\n"))
        if m and not m.group(2).startswith("/"):
            line = f"{m.group(1)}{fx}/{m.group(2)}\n"
        out.append(line)
    return "".join(out)

def fresh(fx):
    if os.path.exists(fx):
        shutil.rmtree(fx)
    subprocess.run(["cp", "-a", SRC, fx], check=True)
    # Steady-state global updates, as conftest.py seed_updates_record does.
    seed = {}
    for line in repos(fx).splitlines():
        m = re.match(r"^location\s*=\s*(\S.*)$", line)
        if not m:
            continue
        upd = os.path.join(m.group(1), "profiles", "updates")
        if os.path.isdir(upd):
            for name in sorted(os.listdir(upd)):
                q = os.path.join(upd, name)
                if not name.startswith(".") and os.path.isfile(q) and not os.path.islink(q):
                    seed[q] = int(os.stat(q).st_mtime)
    edb = os.path.join(fx, "var/cache/edb"); os.makedirs(edb, exist_ok=True)
    mt = os.path.join(edb, "mtimedb")
    try:
        data = json.load(open(mt))
    except Exception:
        data = {}
    data.setdefault("updates", {}).update(seed)
    open(mt, "w").write(json.dumps(data, ensure_ascii=False, indent="	", sort_keys=True))

def run(job, outdir):
    jid = job["id"]
    base = os.path.join(WORK, jid)
    fx = os.path.join(base, "fx")
    os.makedirs(os.path.join(base, "tmp"), exist_ok=True)
    os.makedirs(os.path.join(base, "etc"), exist_ok=True)
    if not os.path.islink(os.path.join(base, "etc", "make.profile")):
        os.symlink(fx + "/repo/profiles/default", os.path.join(base, "etc", "make.profile"))
    args = ["--ignore-default-opts"] + [a.replace("<FIXTURES>", fx) for a in job["args"]]
    for pm in ("portage", "portuale"):
        fresh(fx)
        if job.get("world_extra"):
            with open(os.path.join(fx, "var/lib/portage/world"), "a") as w:
                for a in job["world_extra"]:
                    w.write(a + "\n")
        env = {k: v for k, v in os.environ.items() if k not in STRIP}
        env.update(PORTAGE_CONFIGROOT=fx, ROOT=fx, PORTAGE_RUNNING_ROOT=fx, DISTDIR=fx + "/distfiles",
                   CLEAN_DELAY="0", PORTAGE_TMPDIR=base + "/tmp", PYTHONHASHSEED="0",
                   NS_GLOBAL=FILTER_SRC, NS_FX=fx, NS_ETC=base + "/etc")
        for k, v in job.get("env", {}).items():
            env[k] = v.replace("<FIXTURES>", fx)
        if pm == "portage":
            log = os.path.join(outdir, jid + ".trace.jsonl")
            if os.path.exists(log):
                os.remove(log)
            env["SC_TRACE_LOG"] = log
            cmd = [FILTER, TRACE, "emerge"] + args
        else:
            cmd = [FILTER, PORTUALE, "emerge"] + args
        try:
            r = subprocess.run(cmd, env=env, cwd=fx, capture_output=True, text=True, timeout=600, stdin=subprocess.DEVNULL)
            so, se, rc = r.stdout, r.stderr, r.returncode
        except subprocess.TimeoutExpired:
            so, se, rc = "", "TIMEOUT", -999
        so, se = so.replace(fx, "<FX>"), se.replace(fx, "<FX>")
        for ext, val in (("stdout", so), ("stderr", se), ("rc", str(rc))):
            open(os.path.join(outdir, f"{jid}.{pm}.{ext}"), "w").write(val)
    open(os.path.join(outdir, jid + ".argv"), "w").write(json.dumps(
        dict(args=args, env=job.get("env", {}), name=job.get("name"), world_extra=job.get("world_extra", []),
             common_env="PORTAGE_CONFIGROOT=ROOT=PORTAGE_RUNNING_ROOT=<FX> DISTDIR=<FX>/distfiles CLEAN_DELAY=0 "
                        "PYTHONHASHSEED=0 "
                        "cwd=<FX>; both through ns-run.sh (private mount ns: comments-only global repos.conf, /etc/portage = dir with only make.profile -> <FX>/repo/profiles/default, fixture <FX>/var/db at /var/db, fixture edb at /var/cache/edb); portage: python3 -I sc-trace.py ARGS (argv0 emerge); portuale: portuale (argv0 emerge) ARGS")) + "\n")
    shutil.rmtree(base)
    return jid

jobs = json.load(open(sys.argv[1]))
outdir = sys.argv[2]
os.makedirs(outdir, exist_ok=True)
n = int(sys.argv[3]) if len(sys.argv) > 3 else 16
with ThreadPoolExecutor(n) as ex:
    for i, j in enumerate(ex.map(lambda j: run(j, outdir), jobs)):
        if i % 100 == 0:
            print(i, j, flush=True)
