#!/usr/bin/env python3
"""F2 — taxonomy candidates.

Deterministic part: embed each document's card, cluster at several granularities.
LLM part: name and bound each cluster, then propose a merged taxonomy.

Outputs under work/taxonomy/:
  docs.jsonl        per-document embedding + affinity table
  k{N}.md           one candidate taxonomy per granularity
  affinity.md       deterministic skill-affinity cross-tab
"""

import json
import os
import re
import subprocess
import sys
import time
import urllib.request
from collections import Counter, defaultdict

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.environ.get("PORTUALE_DISTILL_REPO") or os.path.dirname(
    os.path.dirname(os.path.dirname(HERE))
)
WORK = os.environ.get("PORTUALE_DISTILL_WORK") or os.path.join(
    os.path.expanduser("~/.cache"), "portuale-distill", "work"
)

OUT = os.path.join(WORK, "taxonomy")
EMB_URL = "http://127.0.0.1:8098/v1/embeddings"
LLM_URL = "http://127.0.0.1:8097/v1/chat/completions"
EMB_MODEL = os.environ.get("PORTUALE_DISTILL_EMB_MODEL") or os.path.join(
    os.path.expanduser("~/.cache"), "portuale-distill", "models", "bge-m3-q8_0.gguf"
)
SKILLS = [
    "slice-delivery", "oracle-probing", "resolver", "ebuild-merge", "tests-harness",
    "code-conventions", "output-contract", "portage-source-map", "docs-and-charter",
]


def server_up(port):
    try:
        return json.load(urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=2))["status"] == "ok"
    except Exception:
        return False


def start_embed_server():
    if server_up(8098):
        return None
    log = open(os.path.join(WORK, "server-emb.log"), "w")
    p = subprocess.Popen(
        ["llama-server", "-m", EMB_MODEL, "--embeddings", "--pooling", "cls",
         "-c", "8192", "-b", "8192", "-ub", "8192", "-ngl", "99", "--host", "127.0.0.1", "--port", "8098"],
        stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, start_new_session=True)
    for _ in range(60):
        time.sleep(2)
        if server_up(8098):
            return p
    raise SystemExit("embedding server did not come up")


def _one(t):
    req = urllib.request.Request(
        EMB_URL, data=json.dumps({"input": [t[:6000]], "model": "bge-m3"}).encode(),
        headers={"Content-Type": "application/json"})
    r = json.load(urllib.request.urlopen(req, timeout=300))
    return r["data"][0]["embedding"]


def embed(texts, batch=4):
    out = []
    for i in range(0, len(texts), batch):
        b = texts[i : i + batch]
        try:
            req = urllib.request.Request(
                EMB_URL, data=json.dumps({"input": [t[:6000] for t in b], "model": "bge-m3"}).encode(),
                headers={"Content-Type": "application/json"})
            r = json.load(urllib.request.urlopen(req, timeout=300))
            out.extend(d["embedding"] for d in r["data"])
        except Exception as e:  # noqa: BLE001
            print(f"  batch {i} failed ({e}); retrying one by one", flush=True)
            for t in b:
                out.append(_one(t))
        if i % 40 == 0:
            print(f"  embedded {i}/{len(texts)}", flush=True)
    return np.array(out, dtype=np.float32)


def ask(prompt, schema=None, max_tokens=900):
    body = {"messages": [{"role": "user", "content": prompt}], "temperature": 0.1,
            "max_tokens": max_tokens, "chat_template_kwargs": {"enable_thinking": False}}
    if schema:
        body["response_format"] = {"type": "json_schema", "json_schema": {"name": "x", "schema": schema}}
    req = urllib.request.Request(LLM_URL, data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    r = json.load(urllib.request.urlopen(req, timeout=900))
    return r["choices"][0]["message"]["content"]


def doc_text(cards):
    parts = []
    for c in sorted(cards, key=lambda x: -x.get("_kept", 0))[:4]:
        parts.append("; ".join(c["topics"]))
        for it in c["claims"][:3]:
            parts.append(it["text"])
        pr = c.get("procedure")
        if pr:
            parts.append(pr["name"] + ": " + " ".join(pr["steps"][:4]))
    return " ".join(parts)[:4000]


def main():
    os.makedirs(OUT, exist_ok=True)
    cards = [json.loads(l) for l in open(os.path.join(WORK, "cards_b.jsonl"))]
    cards = [c for c in cards if "_error" not in c]
    by_doc = defaultdict(list)
    for c in cards:
        by_doc[c["path"]].append(c)

    docs = sorted(by_doc)
    texts = [f"{d}\n" + doc_text(by_doc[d]) for d in docs]
    print(f"documents: {len(docs)}", flush=True)

    p = start_embed_server()
    X = embed(texts)
    X /= np.linalg.norm(X, axis=1, keepdims=True) + 1e-9
    print(f"embedded: {X.shape}", flush=True)
    if p:
        p.terminate()

    from sklearn.cluster import AgglomerativeClustering

    with open(os.path.join(OUT, "docs.jsonl"), "w") as f:
        for d, v in zip(docs, X):
            f.write(json.dumps({"path": d, "vec": [round(float(x), 5) for x in v]}) + "\n")

    # deterministic affinity cross-tab
    aff = Counter()
    for c in cards:
        for s in c["skills"]:
            aff[(s, c["path"])] += 1
    with open(os.path.join(OUT, "affinity.md"), "w") as f:
        f.write("# Skill-affinity votes (from card `skills`, no clustering)\n\n")
        per_skill = Counter()
        for (s, _), n in aff.items():
            per_skill[s] += n
        f.write("| skill | votes | documents |\n|---|---|---|\n")
        for s, n in per_skill.most_common():
            f.write(f"| {s} | {n} | {len({d for (ss, d) in aff if ss == s})} |\n")

    NAME_SCHEMA = {
        "type": "object",
        "properties": {
            "clusters": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"},
                        "name": {"type": "string"},
                        "definition": {"type": "string"},
                        "excludes": {"type": "string"},
                    },
                    "required": ["id", "name", "definition", "excludes"],
                    "additionalProperties": False,
                },
            }
        },
        "required": ["clusters"],
        "additionalProperties": False,
    }

    for k in (5, 7, 9, 11):
        model = AgglomerativeClustering(n_clusters=k, metric="cosine", linkage="average")
        labels = model.fit_predict(X)
        groups = defaultdict(list)
        for i, lab in enumerate(labels):
            groups[int(lab)].append(i)

        blocks = []
        for lab in sorted(groups):
            idx = groups[lab]
            cen = X[idx].mean(axis=0)
            cen /= np.linalg.norm(cen) + 1e-9
            order = sorted(idx, key=lambda i: -float(X[i] @ cen))
            sample = []
            for i in order[:8]:
                d = docs[i]
                tops = sorted({t for c in by_doc[d] for t in c["topics"]})
                titles = [os.path.basename(d)] + tops[:6]
                sample.append("- " + "; ".join(titles))
            blocks.append(f"CLUSTER {lab} ({len(idx)} docs)\n" + "\n".join(sample))

        prompt = (
            f"Sei un architetto di conoscenza. Ecco {k} cluster di documenti tecnici di `portuale`, "
            "un package manager Gentoo in Rust. Per OGNI cluster: un nome breve (2-4 parole), una "
            "definizione di una frase che dica QUANDO un agente dovrebbe consultarlo, e una frase "
            "che dica cosa NON gli appartiene (il confine con i cluster vicini). Scrivi in inglese.\n\n"
            + "\n\n".join(blocks)
        )
        try:
            raw = ask(prompt, NAME_SCHEMA, 1600)
            named = json.loads(raw)["clusters"]
        except Exception as e:  # noqa: BLE001
            named = []
            print("naming failed:", e, flush=True)

        with open(os.path.join(OUT, f"k{k}.md"), "w") as f:
            f.write(f"# Taxonomy candidate k={k}\n\n")
            for lab in sorted(groups):
                idx = groups[lab]
                info = next((n for n in named if n["id"] == lab), None)
                f.write(f"## {lab}. {info['name'] if info else '(unnamed)'} — {len(idx)} docs\n\n")
                if info:
                    f.write(f"**When:** {info['definition']}\n\n**Excludes:** {info['excludes']}\n\n")
                f.write("\n".join(f"- `{docs[i]}`" for i in sorted(idx)) + "\n\n")
        print(f"k={k} written", flush=True)

    print("done", flush=True)


if __name__ == "__main__":
    main()
