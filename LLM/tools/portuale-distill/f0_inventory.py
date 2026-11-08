#!/usr/bin/env python3
"""F0 — deterministic inventory of the LLM/ corpus.

No LLM involved. Produces, under WORK/:
  inventory.jsonl  one row per file (class, kind, size, hashes, refs)
  chunks.jsonl     heading-aware chunks of the prose corpus
  dupes.md         near-duplicate clusters (minhash)
  summary.md       the numbers the plan is built on

Classes:
  A  kept verbatim (machine-referenced or live working state)
  B  prose to distil into skills
  C  raw evidence (archived out of the tree)
"""

import hashlib
import json
import os
import re
import sys
from collections import Counter, defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.environ.get("PORTUALE_DISTILL_REPO") or os.path.dirname(
    os.path.dirname(os.path.dirname(HERE))
)
WORK = os.environ.get("PORTUALE_DISTILL_WORK") or os.path.join(
    os.path.expanduser("~/.cache"), "portuale-distill", "work"
)
LLM = os.path.join(REPO, "LLM")

# Files that tooling or the agent rulebook reads: never touched.
A_EXACT = {
    "LLM/README.md",
    "LLM/agent-context.md",
    "LLM/backlog-tasks.md",
    "LLM/backlog-tasks-2026-10.md",
    "LLM/scope-backlog.md",
    "LLM/scope-cuts.md",
    "LLM/what-this-proves.md",
    "LLM/renames.tsv",
    "LLM/portage-quirks.md",
}
A_PREFIX = ("LLM/code-notes/", "LLM/tools/")


def rel(p):
    return os.path.relpath(p, REPO)


def klass(relpath):
    if relpath in A_EXACT or relpath.startswith(A_PREFIX):
        return "A"
    if relpath.startswith("LLM/evidence/"):
        return "C"
    return "B"


def kind(relpath):
    n = os.path.basename(relpath)
    if relpath.startswith("LLM/code-notes/"):
        return "code-note"
    if relpath.startswith("LLM/tools/"):
        return "tool"
    if relpath.startswith("LLM/evidence/"):
        return "evidence"
    if n.endswith(".review.md") or ".review" in n:
        return "review"
    if n.endswith(".muse.md"):
        return "plan-alt"
    if n.endswith(".opus.md") or re.match(r"\d\d\.\d+-", n):
        return "plan"
    if n.endswith(".errata.md"):
        return "errata"
    if relpath in A_EXACT:
        return "state"
    if relpath.endswith(".md"):
        return "reference"
    return "data"


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


# ---------- chunking ----------
H = re.compile(r"^(#{1,6})\s+(.*)$")


def chunk_markdown(text, path, target=600, hard=900):
    lines = text.splitlines()
    out, buf, heads = [], [], []

    def flush():
        nonlocal buf
        if not buf:
            return
        words = " ".join(buf).split()
        if not words:
            buf = []
            return
        out.append(
            {
                "path": path,
                "heading": " > ".join(heads) if heads else "",
                "text": "\n".join(buf).strip(),
                "words": len(words),
            }
        )
        buf = []

    for ln in lines:
        m = H.match(ln)
        if m:
            flush()
            lvl = len(m.group(1))
            heads = heads[: lvl - 1] + [m.group(2).strip()]
            continue
        buf.append(ln)
        # soft cap inside one section
        if sum(len(x.split()) for x in buf[-400:]) > hard:
            flush()
    flush()
    return out


def pack_sections(chs, target=600, hard=900):
    """Greedily pack short heading sections into ~target-word chunks."""
    out, buf, n = [], [], 0

    def flush():
        nonlocal buf, n
        if buf:
            out.append(
                {
                    "path": buf[0]["path"],
                    "heading": buf[0]["heading"],
                    "text": "\n\n".join(b["text"] for b in buf),
                    "words": n,
                }
            )
            buf, n = [], 0

    for c in chs:
        if c["words"] >= target:
            flush()
            out.extend(split_long(c, target, hard))
            continue
        if n + c["words"] > hard:
            flush()
        if not buf:
            buf, n = [c], c["words"]
        else:
            buf.append(c)
            n += c["words"]
        if n >= target:
            flush()
    flush()
    return out


def split_long(ch, target=600, hard=900):
    if ch["words"] <= hard:
        return [ch]
    paras = re.split(r"\n\s*\n", ch["text"])
    out, buf, n = [], [], 0
    for p in paras:
        w = len(p.split())
        if n + w > target and buf:
            out.append({**ch, "text": "\n\n".join(buf), "words": n})
            buf, n = [], 0
        buf.append(p)
        n += w
    if buf:
        out.append({**ch, "text": "\n\n".join(buf), "words": n})
    return out


# ---------- minhash ----------
def shingles(text, k=5):
    w = re.findall(r"\w+", text.lower())
    return {" ".join(w[i : i + k]) for i in range(max(0, len(w) - k + 1))}


MASK = (1 << 64) - 1


def minhash(sh, n=128):
    hv = [int(hashlib.blake2b(s.encode(), digest_size=8).hexdigest(), 16) for s in sh]
    if not hv:
        return (MASK,) * n
    out = []
    for i in range(n):
        a = (i * 2654435761 + 1) | 1
        b = i * 40503
        out.append(min(((h * a + b) & MASK) for h in hv))
    return tuple(out)


def jacc(a, b):
    return sum(1 for x, y in zip(a, b) if x == y) / len(a)


def main():
    os.makedirs(WORK, exist_ok=True)
    files = []
    for dirpath, dirnames, filenames in os.walk(LLM):
        dirnames[:] = sorted(dirnames)
        for fn in sorted(filenames):
            p = os.path.join(dirpath, fn)
            files.append(p)

    # Reference counts: how many prose files mention each path / basename.
    prose = [p for p in files if p.endswith(".md")]
    mentions = Counter()
    for p in prose:
        try:
            t = open(p, errors="ignore").read()
        except OSError:
            continue
        for m in set(re.findall(r"(?:LLM/)?[\w./-]+\.md", t)):
            mentions[m.split("/")[-1]] += 1

    inv, chunks = [], []
    for p in files:
        r = rel(p)
        k = klass(r)
        if os.path.islink(p) and not os.path.exists(p):
            row = {
                "path": r,
                "class": k,
                "kind": kind(r),
                "bytes": 0,
                "sha256": None,
                "refs": mentions.get(os.path.basename(r), 0),
                "broken_symlink": True,
            }
            inv.append(row)
            continue
        size = os.path.getsize(p)
        row = {
            "path": r,
            "class": k,
            "kind": kind(r),
            "bytes": size,
            "sha256": sha256(p),
            "refs": mentions.get(os.path.basename(r), 0),
        }
        if r.endswith(".md"):
            t = open(p, errors="ignore").read()
            row["words"] = len(t.split())
            chunks.extend(pack_sections(chunk_markdown(t, r)))
        inv.append(row)

    with open(os.path.join(WORK, "inventory.jsonl"), "w") as f:
        for row in inv:
            f.write(json.dumps(row) + "\n")

    for i, c in enumerate(chunks):
        c["id"] = f"c{i:05d}"
    with open(os.path.join(WORK, "chunks.jsonl"), "w") as f:
        for c in chunks:
            f.write(json.dumps(c) + "\n")

    # near-duplicates among B-class prose
    bdocs = [r for r in inv if r["class"] == "B" and r.get("words", 0) > 80]
    sigs = {}
    for r in bdocs:
        t = open(os.path.join(REPO, r["path"]), errors="ignore").read()
        sigs[r["path"]] = minhash(shingles(t))
    pairs = []
    keys = list(sigs)
    for i in range(len(keys)):
        for j in range(i + 1, len(keys)):
            s = jacc(sigs[keys[i]], sigs[keys[j]])
            if s >= 0.5:
                pairs.append((round(s, 3), keys[i], keys[j]))
    pairs.sort(reverse=True)

    with open(os.path.join(WORK, "dupes.md"), "w") as f:
        f.write("# Near-duplicate prose (minhash 5-gram, Jaccard >= 0.5)\n\n")
        for s, a, b in pairs:
            f.write(f"- {s:.3f}  `{a}`  <>  `{b}`\n")
        f.write(f"\n{pairs and len(pairs) or 0} pairs\n")

    by_class = Counter(r["class"] for r in inv)
    by_kind = Counter(r["kind"] for r in inv)
    bytes_by_class = Counter()
    for r in inv:
        bytes_by_class[r["class"]] += r["bytes"]
    b_words = sum(r.get("words", 0) for r in inv if r["class"] == "B")
    a_words = sum(r.get("words", 0) for r in inv if r["class"] == "A")
    with open(os.path.join(WORK, "summary.md"), "w") as f:
        f.write("# F0 inventory summary\n\n")
        f.write(f"files: {len(inv)}   bytes: {sum(r['bytes'] for r in inv)}\n\n")
        f.write("## by class\n\n| class | files | bytes | words |\n|---|---|---|---|\n")
        for c in "ABC":
            f.write(
                f"| {c} | {by_class[c]} | {bytes_by_class[c]} | "
                f"{b_words if c == 'B' else a_words if c == 'A' else '-'} |\n"
            )
        f.write("\n## by kind\n\n| kind | files |\n|---|---|\n")
        for k, v in by_kind.most_common():
            f.write(f"| {k} | {v} |\n")
        f.write(f"\nchunks: {len(chunks)}  (avg {sum(c['words'] for c in chunks)/max(1,len(chunks)):.0f} words)\n")
        f.write(f"near-dup pairs: {len(pairs)}\n")
        f.write("\n## B-class files by reference count (top 30)\n\n")
        for r in sorted((r for r in inv if r["class"] == "B"), key=lambda x: -x["refs"])[:30]:
            f.write(f"- {r['refs']:3d}  `{r['path']}` ({r['words']}w)\n")
    print(open(os.path.join(WORK, "summary.md")).read())


if __name__ == "__main__":
    sys.exit(main())
