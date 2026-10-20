#!/usr/bin/env python3
"""b453 probes: run one argv under a pty (asciinema) or piped, on a staged fixture tree."""
import json, os, shutil, subprocess, sys
from pathlib import Path
sys.path.insert(0, "/home/vivo/repo/PORTUALE/pmtest/scripts")
sys.path.insert(0, "/home/vivo/repo/PORTUALE/pmtest/managers")
import cast_record, cast_cells

SCR = Path("/var/tmp/b453")
OUT = Path("/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b453/casts")
OUT.mkdir(parents=True, exist_ok=True)
LINK = SCR / "bin" / "emerge"
LINK.parent.mkdir(parents=True, exist_ok=True)
if not LINK.exists():
    LINK.symlink_to("/home/vivo/repo/PORTUALE/portuale/target/release/portuale")
PMS = {"portage": "/usr/sbin/emerge", "portuale": str(LINK)}

def stage(name, warm=True, pm="portage"):
    d = SCR / f"stage-{name}"
    shutil.rmtree(d, ignore_errors=True)
    d.mkdir(parents=True)
    root = cast_cells.stage_fixtures(d)
    if warm:  # a non-version action so _global_updates runs once and commits mtimedb
        env = cast_cells.fixture_env(root)
        subprocess.run([PMS[pm], "--ignore-default-opts", "--pretend", "dev-libs/newpkg"],
                       env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return root

def env_for(root, color=False, extra=None, drop=()):
    env = cast_cells.fixture_env(root, color=color)
    for k in drop:
        env.pop(k, None)
    env.update(extra or {})
    return env

def tty(name, pm, args, root, **kw):
    argv = [PMS[pm], "--ignore-default-opts", *args]
    path, rc = cast_record.record(argv, OUT / f"{name}.{pm}.cast", env=env_for(root, **kw))
    data = "".join(json.loads(l)[2] for l in path.read_text().splitlines()[1:]
                   if l.strip() and json.loads(l)[1] == "o")
    n = sum(1 for l in path.read_text().splitlines()[1:] if l.strip() and json.loads(l)[1] == "o")
    return argv, rc, data, n

def tty_sh(name, pm, shcmd, root, **kw):
    argv = ["sh", "-c", shcmd.replace("EMERGE", PMS[pm] + " --ignore-default-opts")]
    path, rc = cast_record.record(argv, OUT / f"{name}.{pm}.cast", env=env_for(root, **kw))
    data = "".join(json.loads(l)[2] for l in path.read_text().splitlines()[1:]
                   if l.strip() and json.loads(l)[1] == "o")
    return argv, rc, data, None

def pipe(name, pm, args, root, **kw):
    argv = [PMS[pm], "--ignore-default-opts", *args]
    p = subprocess.run(argv, env=env_for(root, **kw), stdin=subprocess.DEVNULL,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    (OUT / f"{name}.{pm}.pipe.stdout").write_bytes(p.stdout)
    (OUT / f"{name}.{pm}.pipe.stderr").write_bytes(p.stderr)
    return argv, p.returncode, p.stdout.decode(errors="replace") + "\n<<STDERR>>\n" + p.stderr.decode(errors="replace"), None

NOISE = ("!!! Repository '", "!!! Set 'masters")
def show(label, res, root):
    argv, rc, data, n = res
    data = data.replace(str(root), "$ROOT")
    keep = "\n".join(l for l in data.split("\n") if not l.lstrip("\r").startswith(NOISE))
    print(f"=== {label}\nargv: {argv}\nrc={rc} events={n}\n{keep!r}\n")
