#!/usr/bin/env python3
"""usage: cells.py <emerge> <signal-mode> -- run slowprobe in a pty, send ^C (mode ctrlc) or SIGTERM to the emerge pid (mode term)."""
import os, pty, sys, time, signal, select
S = os.environ.get("S7", "/var/tmp/b411")
em, mode = sys.argv[1], sys.argv[2]
env = dict(os.environ, EMERGE_LOG_DIR=S+"/log", PORTAGE_CONFIGROOT=S, PORTAGE_TMPDIR=S+"/tmp", ROOT=S+"/root", DISTDIR=S+"/dist")
os.makedirs(S+"/log", exist_ok=True)
pid, fd = pty.fork()
if pid == 0:
    os.execvpe(em, [em, "dev-util/slowprobe"], env)
out = b""
def drain(t):
    global out
    end = time.time()+t
    while time.time() < end:
        r,_,_ = select.select([fd],[],[],0.2)
        if r:
            try: d = os.read(fd, 4096)
            except OSError: return False
            if not d: return False
            out += d
    return True
drain(15)
if mode == "ctrlc": os.write(fd, b"\x03")
else: os.kill(pid, signal.SIGTERM)
drain(float(os.environ.get("W","8")))
try: _, st = os.waitpid(pid, os.WNOHANG)
except ChildProcessError: st = None
time.sleep(1)
print(out.decode(errors="replace"))
print("status", st, "exit", (os.WEXITSTATUS(st) if st and os.WIFEXITED(st) else None), "sig", (os.WTERMSIG(st) if st and os.WIFSIGNALED(st) else None))
sp = sum(1 for d in os.listdir("/proc") if d.isdigit() and open(f"/proc/{d}/cmdline","rb").read().replace(b"\0",b" ").strip()==b"sleep 300")
print("sleep survivors:", sp)
