#!/usr/bin/env python3
"""#317 S0: reduce an `strace -f -y` log to the VDB calls that change
state or lock, in order: `pid syscall path [args] = result`.
Usage: 317-s0-reduce.py <step>.vdb  (lines already filtered to the VDB, ROOT stripped)"""
import re, sys
MUT = ("mkdir", "rmdir", "unlink", "unlinkat", "rename", "renameat", "renameat2", "link", "linkat",
       "symlink", "symlinkat", "utimensat", "chmod", "fchmod", "fchmodat", "chown", "fchown", "lchown",
       "fchownat", "truncate", "ftruncate", "fcntl", "flock", "copy_file_range", "sendfile", "write",
       "pwrite64", "fsync", "fdatasync", "setxattr", "lsetxattr", "fsetxattr", "openat", "open")
LOCK = re.compile(r"F_SETLKW?|F_OFD_SETLKW?|LOCK_")
WR = re.compile(r"O_WRONLY|O_RDWR|O_CREAT|O_TRUNC|O_APPEND")
for line in open(sys.argv[1]):
    m = re.match(r"(\d+)\s+\S+\s+(\w+)\((.*)", line)
    if not m:
        continue
    pid, sc, rest = m.groups()
    if sc not in MUT:
        continue
    if sc in ("openat", "open") and not WR.search(rest):
        continue
    if sc == "fcntl" and not LOCK.search(rest):
        continue
    # `call(<dirfd path>, "name", ...)`: join a relative name to its dir fd
    rel = re.match(r'\s*\d+<(/var/db/pkg[^>]*)>, "([^"/][^"]*)"', rest)
    paths = re.findall(r'"(/var/db/pkg[^"]*)"|<(/var/db/pkg[^>]*)>', rest)
    p = [a or b for a, b in paths]
    if rel:
        p = [rel.group(1) + "/" + rel.group(2)] + p[1:]
    if sc in ("write", "pwrite64") and not re.match(r'\s*\d+<', rest):
        continue
    res = re.search(r"\)\s+=\s+(.*)$", line)
    res = res.group(1).strip() if res else "<unfinished>"
    res = re.sub(r"<[^>]*>", "", res)
    flags = " ".join(re.findall(r"\b(O_[A-Z]+(?:\|O_[A-Z]+)*|F_\w+|LOCK_\w+|0[0-7]{3,4}|AT_\w+)\b", rest))
    print(f"{pid} {sc} {' -> '.join(dict.fromkeys(p))} {flags} = {res}".replace("  ", " "))
