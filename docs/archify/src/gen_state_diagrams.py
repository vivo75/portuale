import json,sys
REV="2055f4dc3e08fc79060527b2fc9db663c24baa2d"
import os
OUT=os.path.dirname(os.path.abspath(__file__))+"/"
def S(i,t,l,sub=None,tag=None,src=None):
    d={"id":i,"type":t,"label":l}
    if sub:d["sublabel"]=sub
    if tag:d["tag"]=tag
    if src:d["sources"]=[{"path":p,"line":n} for p,n in src]
    return d
def T(a,b,label=None,v=None):
    d={"id":f"{a}-{b}","from":a,"to":b}
    if label:d["label"]=label
    if v:d["variant"]=v
    return d
def W(name,title,sub,main,states,trans,cards):
    j={"schema_version":3,"diagram_type":"lifecycle",
     "meta":{"title":title,"subtitle":sub,"output":f"docs/archify/state-{name}.html","quality_profile":"showcase",
       "repository":{"url":"git@github.com:vivo75/portuale.git","link_mode":"local-only","revision":REV}},
     "mainPath":main,"states":states,"transitions":trans,"cards":cards}
    json.dump(j,open(OUT+f"state-{name}.json","w"),indent=1)
P="crates/portuale/src/"
R="crates/portage-repo/src/resolve/"
FAIL="security"

W("phases","Ebuild phase lifecycle","One phase of the chain: skip gates, run, stamp",
 ["requested","gate","running","stamped","qa","done"],
 [S("requested","start","Requested","phase C asked for","chain",[(P+"ebuild_phases/mod.rs",125)]),
  S("gate","decision","Skip gates","pretend, no-op, stamp","guard",[(P+"ebuild_phases/commands.rs",256)]),
  S("skipped","neutral","Skipped","no spawn, no stamp write","skip",[(P+"ebuild_phases/stamp_skip.rs",42)]),
  S("running","active","Running","bash __ebuild_main","spawn",[(P+"ebuild_phases/commands.rs",275)]),
  S("stamped","active","Stamped","bash writes .<phase>ed","state",[(P+"ebuild_phases/stamp_skip.rs",34)]),
  S("qa","active","Post-install","QA, metadata, soname deps","install only",[(P+"ebuild_phases/commands.rs",297)]),
  S("done","success","Done","chain complete","terminal"),
  S("failed","failure","Failed","status != 0, chain stops","stop",[(P+"ebuild_phases/commands.rs",286)])],
 [T("requested","gate"),T("gate","skipped","pretend / no-op / stamped"),T("skipped","gate","next phase","dashed"),
  T("gate","running","run"),T("running","stamped","ok"),T("running","failed","status != 0",FAIL),
  T("stamped","gate","next phase","dashed"),T("stamped","qa","ok"),T("qa","done")],
 [{"dot":"cyan","title":"Where the state lives","items":["Stamp files in the build dir: .unpacked, .configured, .installed ...","Bash writes them after the phase body; Rust only reads them"]},
  {"dot":"amber","title":"noauto","items":["install ignores its stamp and always re-runs","elog's EBUILD_PHASES only orders messages; it is not this chain"]}])

W("backtracking","Backtracking search","Driver loop over mask and config choices",
 ["root","pass","decision","settled"],
 [S("root","start","Root node","initial params","DFS",[(R+"backtracker.rs",94)]),
  S("pass","active","Pass","run_pass on one node","walk",[(R+"driver.rs",79)]),
  S("decision","decision","Decision","Settle, Feedback, DeadEnd","collect",[(R+"driver.rs",224)]),
  S("feedback","active","Feedback","child nodes added","guards",[(R+"backtracker.rs",110)]),
  S("stack","waiting","Unexplored","DFS stack","queued",[(R+"backtracker.rs",143)]),
  S("fallback","active","Best-run fallback","deepest terminal node","budget",[(R+"driver.rs",313)]),
  S("settled","success","Settled","result assembled","terminal"),
  S("error","failure","Root pass error","returned as Err","stop",[(R+"driver.rs",94)])],
 [T("root","pass"),T("pass","decision","pass ran"),T("decision","settled","Settle"),
  T("decision","feedback","Feedback"),T("feedback","stack","add_node accepts"),T("stack","pass","pop","dashed"),
  T("decision","fallback","DeadEnd, budget out"),T("stack","fallback","stack empty","dashed"),
  T("fallback","settled"),T("pass","error","root pass fails",FAIL)],
 [{"dot":"cyan","title":"Two caps","items":["Mask-step cap on node creation: max(1, backtrack_max / 2)","Restart cap in the driver: restarts >= backtrack_max (default 10)"]},
  {"dot":"amber","title":"Feedback kinds","items":["Config, RevDep, Circular add no mask step; SlotConflict and MissingDep add one","--backtrack=0 settles the root pass, except RevDep which re-runs as a feed pass"]}])

W("resolver-pass","Resolver pass","One pass over the queue; phase is implicit in queue contents",
 ["seeding","walking","disjunction","finish","result"],
 [S("seeding","start","Seeding","args, greedy, rebuild seeds","queue",[(R+"pass/seed_queue.rs",15)]),
  S("walking","active","Pop and step","queue, held_blocks","loop",[(R+"pass/pop_queue_item.rs",10)]),
  S("outcome","decision","Outcome","resolve, then fallbacks","verdict",[(R+"pass/outcome_fallbacks.rs",119)]),
  S("graphed","active","Graphed","slot entry pushed","node",[(R+"pass/graph_entry.rs",151)]),
  S("dropped","neutral","Dropped or unresolved","Break: entry recorded or skipped","terminal step",[(R+"pass/unresolved_dep.rs",777)]),
  S("disjunction","active","|| groups","queue and held drained","deferred",[(R+"pass/mod.rs",138)]),
  S("finish","active","Finish","required_by, slot conflicts, blockers","post-walk",[(R+"pass/mod.rs",187)]),
  S("result","success","PassResult","data for the driver","terminal",[(R+"pass/assemble_pass.rs",15)]),
  S("error","failure","Err","top-level unsatisfied, REQUIRED_USE","abort",[(R+"pass/top_level_unsatisfied.rs",226)])],
 [T("seeding","walking","seeds_left set"),T("walking","outcome","pop"),T("outcome","graphed","version chosen"),
  T("graphed","walking","deps enqueued","dashed"),T("outcome","dropped","no version, or Break"),T("dropped","walking","next item","dashed"),
  T("walking","disjunction","drained"),T("disjunction","walking","choice queued","dashed"),
  T("disjunction","finish","stack empty"),T("finish","result"),
  T("outcome","error","top-level NVC",FAIL),T("finish","error","REQUIRED_USE",FAIL)],
 [{"dot":"cyan","title":"No state enum","items":["Phase is carried by which of queue, held_blocks and disjunctive_stack is non-empty","States here are inferred groupings, not named types"]},
  {"dot":"amber","title":"Break is overloaded","items":["Break means both dropped on purpose and fully handled","The driver, not the pass, decides what re-runs"]}])

W("resume","Merge-list entry and resume","State is derived from the saved list and CONTENTS",
 ["planned","saved","merging","merged"],
 [S("planned","start","Planned","in the merge plan","entry",[(P+"pretend/merge_run.rs",93)]),
  S("saved","active","Saved","in mtimedb resume list","persisted",[(P+"mtimedb.rs",903)]),
  S("merging","active","Merging","build and merge","running",[(P+"emerge_build/source_merge.rs",215)]),
  S("merged","success","Merged","removed from the list","terminal",[(P+"emerge_build/source_merge.rs",250)]),
  S("pf","neutral","Postinst failed","landed, kept in list","partial",[(P+"emerge_build/source_merge.rs",259)]),
  S("failed","failure","Failed","stays in the saved list","stop",[(P+"pretend/resume.rs",64)]),
  S("dropped","neutral","Dropped","dependent of a failure","keep-going",[(P+"emerge_build/source_merge.rs",276)])],
 [T("planned","saved","up-front save"),T("saved","merging","merge starts"),T("merging","merged","ok"),
  T("merging","pf","postinst fails"),T("merging","failed","error",FAIL),
  T("failed","dropped","--keep-going","dashed"),T("failed","saved","--resume","dashed"),
  T("pf","saved","--resume","dashed"),T("dropped","saved","--resume","dashed")],
 [{"dot":"cyan","title":"Persistence","items":["mtimedb resume and resume_backup; a success removes its own entry","--resume always runs with keep_going false"]},
  {"dot":"amber","title":"Traps found","items":["read_resume_list can promote the backup, so a read writes","An empty up-front save writes an empty list that does not parse"]}])

W("process-cancel","Phase process cancellation","One spawned phase child and the two kill paths",
 ["unspawned","spawned","registered","waiting","reaped"],
 [S("unspawned","start","Not started","setsid for NewSession","child",[(P+"ebuild_phases/scheduler_registry.rs",122)]),
  S("spawned","active","Spawned","entered in LIVE map","signal path",[(P+"ebuild_phases/scheduler_registry.rs",141)]),
  S("registered","active","Registered","pgids or pids set","registry",[(P+"ebuild_phases/scheduler_registry.rs",144)]),
  S("waiting","waiting","Waiting","child.wait, no timeout","blocking",[(P+"ebuild_phases/scheduler_registry.rs",164)]),
  S("reaped","success","Reaped","removed from both sets","terminal",[(P+"ebuild_phases/scheduler_registry.rs",166)]),
  S("killed","failure","SIGTERM sent","no SIGKILL on either path","kill",[(P+"ebuild_phases/scheduler_registry.rs",103)]),
  S("exited","neutral","portuale exits","128 + signal after 60 s grace","signal",[(P+"ebuild_phases/signal_exit.rs",125)])],
 [T("unspawned","spawned","spawn"),T("spawned","registered","guard present"),T("registered","waiting"),T("waiting","reaped","child exits"),
  T("registered","killed","already terminated",FAIL),T("waiting","killed","kill",FAIL),
  T("killed","reaped","child exits","dashed"),T("killed","exited","grace expired","dashed")],
 [{"dot":"cyan","title":"Two paths","items":["Registry: per scheduler call, thread-local, fires on a non-keep-going failure","Signal watcher: process-wide, fires on SIGINT or SIGTERM, covers every tracked child"]},
  {"dot":"amber","title":"Open","items":["terminated is set only on the failure path, never by ^C","Pid reuse between snapshot and kill was not evaluated"]}])

W("merge-path","Merge path of one package","Source merge: image to ROOT, old unmerged, entry published",
 ["built","prepared","written","replaced","published","finalised"],
 [S("built","start","Built","install done","image",[(P+"ebuild_merge/mod.rs",185)]),
  S("prepared","active","Prepared","instprep, checks","checks",[(P+"ebuild_merge/after_install.rs",14)]),
  S("written","active","Written","entry + files","vdb + ROOT",[(P+"ebuild_merge/after_install.rs",174),(P+"ebuild_merge/after_install.rs",202)]),
  S("replaced","active","Replaced","old unmerged","same slot",[(P+"ebuild_merge/after_install.rs",264)]),
  S("published","active","Published","entry live","commit",[(P+"ebuild_merge/after_install.rs",280)]),
  S("finalised","success","Finalised","env_update, clean","terminal",[(P+"ebuild_merge/after_install.rs",381)]),
  S("pf","neutral","Postinst failed","merge stays","partial",[(P+"ebuild_merge/after_install.rs",373)]),
  S("failed","failure","Failed","no rollback","stop",[(P+"ebuild_merge/tree.rs",90)])],
 [T("built","prepared"),T("prepared","written","ok"),T("written","replaced"),
  T("replaced","published"),T("published","finalised","ok"),T("published","pf","postinst fails"),
  T("prepared","failed","abort",FAIL),T("written","failed","error",FAIL)],
 [{"dot":"cyan","title":"Order","items":["New image first, old unmerge second; the new entry is renamed live last on the files backend","Written = -MERGING- entry, preinst, build-info, files on ROOT, CONTENTS sealed; a preinst or write error leaves a pending entry or partial ROOT","Files are never opened for write: a temp sibling is renamed over the target"]},
  {"dot":"amber","title":"Doc drift","items":["SKILL.md says the VDB lock is held for the whole merge; the code locks one counter tick","Failed states leave a pending entry or partial ROOT; only a manual vdb sweep cleans up"]}])
