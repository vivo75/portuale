"""Summarise census results: every job whose Portage or portuale output has a slot conflict,
with the handler trace. Usage: python3 -I analyze.py <census dir>"""
import glob, json, os, re, sys
d = sys.argv[1]
rows = []
for argv in sorted(glob.glob(os.path.join(d, "*.argv"))):
    jid = os.path.basename(argv)[:-5]
    meta = json.loads(open(argv).read())
    def rd(pm, ext):
        try: return open(os.path.join(d, f"{jid}.{pm}.{ext}")).read()
        except OSError: return ""
    po = rd("portage", "stdout") + rd("portage", "stderr")
    uo = rd("portuale", "stdout") + rd("portuale", "stderr")
    trace = []
    tp = os.path.join(d, jid + ".trace.jsonl")
    if os.path.exists(tp):
        trace = [json.loads(l) for l in open(tp)]
    has_p = "resulting in a slot conflict" in po
    has_u = "resulting in a slot conflict" in uo
    stop = [t for t in trace if t["ev"] == "need_config_change" and t["slot_arm_stop"]]
    built = [t for t in trace if t["ev"] == "need_config_change" and t["built_here"]]
    shows = [t for t in trace if t["ev"] == "show_slot_collision_notice"]
    bt = [t for t in trace if t["ev"] == "backtrack_depgraph"]
    expl = [s["handler"]["explanation"] for s in shows if s["handler"] and s["handler"]["explanation"]]
    if not (has_p or has_u or built or shows):
        continue
    def cnt(o):
        m = re.search(r"\(backtrack: (\d+)/(\d+)\)", o); return m.group(1) if m else "-"
    rows.append(dict(id=jid, args=meta["args"], world=meta.get("world_extra", []), env=meta.get("env", {}), name=meta.get("name"),
        rc_p=rd("portage", "rc"), rc_u=rd("portuale", "rc"), bt_p=cnt(po), bt_u=cnt(uo),
        conflict_p=has_p, conflict_u=has_u,
        stop=[(s["allow_backtracking"], s["handler"]["changes"]) for s in stop],
        built=[dict(allow_bt=b["allow_backtracking"], v=b["handler"]["is_a_version_conflict"], u=b["handler"]["conflict_is_unspecific"], nsol=b["handler"]["n_solutions"], conflicts=b["handler"]["conflicts"]) for b in built],
        shown=[dict(reused=s["handler_reused"], v=s["handler"]["is_a_version_conflict"], u=s["handler"]["conflict_is_unspecific"], nsol=s["handler"]["n_solutions"], changes=s["handler"]["changes"], expl=s["handler"]["explanation"]) for s in shows],
        backtracked=[b["backtracked"] for b in bt],
        p_explains="It might be possible to solve" in po, p_newuse="Enabling --newuse" in po,
        u_explains="It might be possible to solve" in uo, u_newuse="Enabling --newuse" in uo,
        p_pmask="package.mask to" in po, u_pmask="package.mask to" in uo,
        p_bt30="--backtrack=30" in po, u_bt30="--backtrack=30" in uo))
json.dump(rows, open(os.path.join(d, "..", os.path.basename(d.rstrip("/")) + "-summary.json"), "w"), indent=1)
print(len(rows), "jobs with a slot conflict or a handler")
for r in rows:
    flag = []
    if r["stop"]: flag.append("STOP")
    if r["p_explains"]: flag.append("P-EXPLAIN")
    if r["p_newuse"]: flag.append("P-NEWUSE")
    if r["u_explains"] or r["u_newuse"]: flag.append("U-HANDLERTEXT")
    if r["conflict_p"] != r["conflict_u"]: flag.append("CONFLICT-PRESENCE-DIFF")
    if r["p_bt30"] != r["u_bt30"] and r["conflict_p"] and r["conflict_u"]: flag.append("BT30-DIFF")
    print(r["id"], " ".join(r["args"]), ("world+=" + ",".join(r["world"])) if r["world"] else "", "| rc", r["rc_p"], r["rc_u"], "| bt", r["bt_p"], r["bt_u"], "|", ",".join(flag))
