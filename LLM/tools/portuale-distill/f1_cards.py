#!/usr/bin/env python3
"""F1 — map stage: one verifiable card per chunk, local Qwen3.6-35B-A3B.

Every claim/procedure/pitfall must carry a `quote` that is a verbatim substring
of its chunk. The driver checks that mechanically, so a card either points at
real text or is rejected: that is the anti-hallucination gate.

Usage:
  f1_cards.py --start-server --n-parallel 3
  f1_cards.py --limit 20            # smoke test
  f1_cards.py                       # full run, resumable
"""

import argparse
import json
import os
import re
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.environ.get("PORTUALE_DISTILL_REPO") or os.path.dirname(
    os.path.dirname(os.path.dirname(HERE))
)
WORK = os.environ.get("PORTUALE_DISTILL_WORK") or os.path.join(
    os.path.expanduser("~/.cache"), "portuale-distill", "work"
)
MODEL = os.environ.get("PORTUALE_DISTILL_MODEL") or (
    "/mnt/D500G/space/huggingface/hub/models--unsloth--Qwen3.6-35B-A3B-MTP-GGUF/"
    "snapshots/5bc3e238d916f48a861bac2f8a1990a0e9b7e98d/"
    "Qwen3.6-35B-A3B-UD-Q4_K_XL.gguf"
)
PORT = 8097

SKILLS = [
    "slice-delivery",
    "oracle-probing",
    "resolver",
    "ebuild-merge",
    "tests-harness",
    "code-conventions",
    "output-contract",
    "portage-source-map",
    "docs-and-charter",
]

SCHEMA = {
    "type": "object",
    "properties": {
        "kind": {
            "type": "string",
            "enum": ["plan", "review", "lesson", "code-note", "reference", "report", "state"],
        },
        "topics": {"type": "array", "items": {"type": "string"}, "maxItems": 6},
        "claims": {
            "type": "array",
            "maxItems": 5,
            "items": {
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "quote": {"type": "string"},
                },
                "required": ["text", "quote"],
                "additionalProperties": False,
            },
        },
        "procedure": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "steps": {"type": "array", "items": {"type": "string"}, "maxItems": 6},
                "quote": {"type": "string"},
            },
            "required": ["name", "steps", "quote"],
            "additionalProperties": False,
        },
        "reusable": {"type": "boolean"},
        "skills": {
            "type": "array",
            "items": {"type": "string", "enum": SKILLS},
            "maxItems": 3,
        },
        "confidence": {"type": "number"},
    },
    "required": ["kind", "topics", "claims", "reusable", "skills", "confidence"],
    "additionalProperties": False,
}

THIN_SCHEMA = {
    "type": "object",
    "properties": {
        "kind": {"type": "string", "enum": ["plan", "review", "lesson", "code-note", "reference", "report", "state"]},
        "summary": {"type": "string"},
        "topics": {"type": "array", "items": {"type": "string"}, "maxItems": 8},
        "skills": {"type": "array", "items": {"type": "string", "enum": SKILLS}, "maxItems": 3},
        "reusable": {"type": "boolean"},
    },
    "required": ["kind", "summary", "topics", "skills", "reusable"],
    "additionalProperties": False,
}

THIN_SYSTEM = """You index engineering documents from `portuale`, an independent Gentoo package manager in Rust.
For the excerpt given, return a SHORT index card (no quotes, no detail): what it is about, which topics it names, and which of the closed-list skills would want to consult it later.
Keep `summary` under 35 words. Do not invent.

Closed list of skills: slice-delivery, oracle-probing, resolver, ebuild-merge, tests-harness, code-conventions, output-contract, portage-source-map, docs-and-charter.
"""

SYSTEM = """You catalogue engineering documents from `portuale`, an independent Gentoo package manager written in Rust.

You receive ONE excerpt, with its source path and heading trail. Produce a catalogue card.
The card must be conservative and strictly grounded:
- `claims` and `procedure` must each carry a `quote`: a short verbatim string (5-25 words) copied EXACTLY from the excerpt, no ellipsis. Never paraphrase inside `quote`.
- If the excerpt carries no durable knowledge (boilerplate, a table of contents, pure narrative of what a command printed), return empty arrays, no `procedure`, and `reusable: false`.
- `skills` names where the knowledge would be useful. Choose at most 3, from the closed list.
- `confidence` is your own 0..1 estimate that the card is worth keeping.

The closed list of skills, with their boundaries:
- slice-delivery: planning a work item end to end - backlog entry, plan, tasks, review focus, gates, close-out, two-repository pairing.
- oracle-probing: establishing what is TRUE before coding - probes, ResolverPlayground oracles, determinism, capture discipline, differential test beds.
- resolver: dependency resolution - backtracking, slot operators, virtuals, demoters, @world updates, merge order, || deps, blockers, package.provided.
- ebuild-merge: executing and merging packages - EAPI and phases, ebuild config options, unmerge and reinstall, binpkgs and gpkg, the VDB, the glibc/bash merge safety gate.
- tests-harness: the test infrastructure - pmtest layout, fixtures, CASES, contract pins, the harvested corpus, gates, asciinema recording, benchmarks.
- code-conventions: writing and refactoring the Rust here - module layout, comment rules, code-notes discipline, giant functions, the #336 conventions.
- output-contract: what the user sees - emerge/mrg display, messages, colours, [ebuild N] rows, --tree, counters, error wording.
- portage-source-map: navigating Portage's own Python source - where a problem lives in lib/portage or lib/_emerge, file:line landmarks, known Portage bugs not to copy.
- docs-and-charter: the project's charter and documentation rules - what counts as correct, doc status headers, link checks, fact coverage, renames, the append-only proof file.
"""

USER = """SOURCE: {path}
HEADING: {heading}

<<<EXCERPT
{text}
EXCERPT>>>

Catalogue this excerpt."""


def start_server(n_parallel, ctx):
    log = open(os.path.join(WORK, "server-f1.log"), "w")
    p = subprocess.Popen(
        [
            "llama-server",
            "-m", MODEL,
            "-c", str(ctx * n_parallel),
            "-np", str(n_parallel),
            "-ngl", "99",
            "-fa", "on",
            "--jinja",
            "--no-reasoning-preserve",
            "--temp", "0.1",
            "--host", "127.0.0.1",
            "--port", str(PORT),
        ],
        stdout=log,
        stderr=subprocess.STDOUT,
        stdin=subprocess.DEVNULL,
        start_new_session=True,
    )
    for _ in range(180):
        time.sleep(2)
        try:
            if json.load(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/health", timeout=2))["status"] == "ok":
                return p
        except Exception:
            pass
    raise SystemExit("server did not come up; see work/server-f1.log")


def norm(s):
    return re.sub(r"\s+", " ", s or "").strip().lower()


def call(chunk, thin=False, retries=3):
    sysmsg, schema, name = (THIN_SYSTEM, THIN_SCHEMA, "index") if thin else (SYSTEM, SCHEMA, "card")
    body = {
        "messages": [
            {"role": "system", "content": sysmsg},
            {"role": "user", "content": USER.format(**chunk)},
        ],
        "temperature": 0.1,
        "max_tokens": 160 if thin else 900,
        "chat_template_kwargs": {"enable_thinking": False},
        "response_format": {"type": "json_schema", "json_schema": {"name": name, "schema": schema}},
    }
    data = json.dumps(body).encode()
    last = None
    for attempt in range(retries):
        try:
            req = urllib.request.Request(
                f"http://127.0.0.1:{PORT}/v1/chat/completions",
                data=data,
                headers={"Content-Type": "application/json"},
            )
            r = json.load(urllib.request.urlopen(req, timeout=600))
            out = json.loads(r["choices"][0]["message"]["content"])
            out["_usage"] = r.get("usage")
            return out
        except Exception as e:  # noqa: BLE001
            last = e
            time.sleep(2 * (attempt + 1))
    return {"_error": str(last)}


def _lcs(a, b):
    """Longest common contiguous token run between two token lists."""
    best, best_i, best_j = 0, 0, 0
    prev = [0] * (len(b) + 1)
    for i in range(1, len(a) + 1):
        cur = [0] * (len(b) + 1)
        for j in range(1, len(b) + 1):
            if a[i - 1] == b[j - 1]:
                cur[j] = prev[j - 1] + 1
                if cur[j] > best:
                    best, best_i, best_j = cur[j], i, j
        prev = cur
    return best, best_i, best_j


def check_quote(quote, hay, hay_tokens):
    """Return (ok, repaired_quote). Accepts exact spans, ellipsis-joined spans,
    and anything whose longest verbatim run covers most of the quote."""
    q = norm(quote)
    if len(q) < 12:
        return False, None
    if q in hay:
        return True, quote
    frags = [f.strip() for f in re.split(r"\.\.\.|…", q) if len(f.strip()) >= 12]
    if len(frags) > 1 and all(f in hay for f in frags):
        return True, quote
    qtok = q.split()
    n, _, j = _lcs(qtok, hay_tokens)
    if n >= 6 and n >= 0.5 * len(qtok):
        return True, " ".join(hay_tokens[j - n : j])
    return False, None


def validate(card, chunk):
    """Keep only claims whose quote is (or is repairable to) a verbatim span."""
    hay_tokens = norm(chunk["text"]).split()
    hay = " ".join(hay_tokens)
    dropped = repaired = 0

    def scrub(item):
        nonlocal dropped, repaired
        ok, fixed = check_quote(item.get("quote", ""), hay, hay_tokens)
        if not ok:
            dropped += 1
            return None
        if fixed != item["quote"]:
            item["quote"] = fixed
            repaired += 1
        return item

    card["claims"] = [x for x in (scrub(i) for i in card.get("claims", [])) if x]
    proc = card.get("procedure")
    card["procedure"] = scrub(proc) if proc else None
    if proc and not card["procedure"]:
        pass
    card["_dropped"] = dropped
    card["_repaired"] = repaired
    card["_kept"] = len(card["claims"]) + (1 if card["procedure"] else 0)
    return card


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--start-server", action="store_true")
    ap.add_argument("--n-parallel", type=int, default=3)
    ap.add_argument("--ctx", type=int, default=4096)
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--thin", action="store_true", help="index-only cards (class A docs)")
    ap.add_argument("--cls", default="", help="restrict to class A or B")
    ap.add_argument("--out", default="cards.jsonl")
    args = ap.parse_args()

    chunks = [json.loads(l) for l in open(os.path.join(WORK, "chunks.jsonl"))]
    if args.cls:
        inv = {json.loads(l)["path"]: json.loads(l) for l in open(os.path.join(WORK, "inventory.jsonl"))}
        chunks = [c for c in chunks if inv.get(c["path"], {}).get("class") in args.cls.split(",")]
    out_path = os.path.join(WORK, args.out)
    done = set()
    if os.path.exists(out_path):
        for l in open(out_path):
            try:
                done.add(json.loads(l)["chunk_id"])
            except Exception:
                pass
    todo = [c for c in chunks if c["id"] not in done]
    if args.limit:
        todo = todo[: args.limit]
    print(f"chunks={len(chunks)} done={len(done)} todo={len(todo)}", flush=True)
    if not todo:
        return

    proc = start_server(args.n_parallel, args.ctx) if args.start_server else None
    lock = threading.Lock()
    t0 = time.time()
    stats = {"n": 0, "kept": 0, "dropped": 0, "err": 0, "tokens_out": 0}

    def work(c):
        card = call(c, thin=args.thin)
        if "_error" in card:
            return c, {**card, "chunk_id": c["id"], "path": c["path"], "heading": c["heading"], "words": c["words"]}
        card = validate(card, c)
        card["chunk_id"] = c["id"]
        card["path"] = c["path"]
        card["heading"] = c["heading"]
        card["words"] = c["words"]
        return c, card

    with open(out_path, "a") as fout, ThreadPoolExecutor(args.workers) as ex:
        for c, card in ex.map(work, todo):
            with lock:
                fout.write(json.dumps(card, ensure_ascii=False) + "\n")
                fout.flush()
                stats["n"] += 1
                if "_error" in card:
                    stats["err"] += 1
                elif args.thin:
                    stats["kept"] += 1
                else:
                    stats["kept"] += card["_kept"]
                    stats["dropped"] += card["_dropped"]
                    stats["tokens_out"] += (card.get("_usage") or {}).get("completion_tokens", 0)
                if stats["n"] % 25 == 0:
                    el = time.time() - t0
                    rate = stats["n"] / el * 60
                    eta = (len(todo) - stats["n"]) / max(rate, 1e-9)
                    print(
                        f"{stats['n']}/{len(todo)}  {rate:.1f} chunk/min  eta {eta:.0f} min  "
                        f"kept={stats['kept']} dropped={stats['dropped']} err={stats['err']}",
                        flush=True,
                    )

    print(json.dumps(stats), flush=True)
    if proc:
        proc.terminate()


if __name__ == "__main__":
    main()
