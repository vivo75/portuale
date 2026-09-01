# Operation block diagrams

Block diagrams for four representative `emerge` invocations, as this
pilot's `portuale` binary actually implements them. Each diagram traces
the real call path through `rust/portuale/src/`; blocks are labelled with
the function that does the work so the diagram doubles as a map into the
source.

| Invocation | Diagram | Kind |
|---|---|---|
| `emerge --pretend @world` | [emerge-pretend-world.md](emerge-pretend-world.md) | read-only resolution + display |
| `emerge -v --getbinpkg=n sys-apps/portage` | [emerge-source-merge.md](emerge-source-merge.md) | source build + merge |
| `emerge -v --getbinpkgonly=y sys-apps/portage` | [emerge-getbinpkgonly.md](emerge-getbinpkgonly.md) | binary-only download + merge |
| `emerge -Cv app-portage/eix` | [emerge-unmerge.md](emerge-unmerge.md) | unmerge (removal) |

## Shared front end

Every `emerge` invocation enters through the same multicall dispatch and
argument/config pipeline before it branches to an action:

```mermaid
flowchart TD
    argv["main() — basename of argv0"]:::entry
    argv -->|"invoked as emerge"| run["emerge::run(args)<br/>(pretend::run, pretend.rs)"]
    run --> help{"--help / -h ?"}
    help -->|yes| printhelp["print_help() → exit 0"]:::done
    help -->|no| parse["parse the full option surface<br/>(emerge_options.rs recognises every<br/>real flag/action by name)"]
    parse --> early{"early standalone action?<br/>--deselect (alone), --list-sets"}
    early -->|yes| earlyact["run_deselect() / run_list_sets()"]:::branch
    early -->|no| cfg["resolve config<br/>find_repos() + resolve_config()<br/>(repos.conf, profile chain, make.conf,<br/>package.mask/.use/.accept_keywords)"]
    cfg --> standalone{"standalone action?<br/>--unmerge/-C, --depclean/-c, --prune,<br/>--config, --search, --info, --resume,<br/>--clean, --rage-clean"}
    standalone -->|yes| action["dispatch to that action's handler<br/>(see per-action diagrams)"]:::branch
    standalone -->|no| expand["expand set tokens in place<br/>@world/@selected/@system/@installed/@&lt;name&gt;<br/>+ apply profiles/updates/ package moves"]
    expand --> resolve["resolve_pretend_graph()<br/>(portage-repo): candidate selection per repo,<br/>recursive slot-aware DEPEND/RDEPEND/BDEPEND walk,<br/>outcome classification, blocker + slot-conflict<br/>reporting, topological merge order"]
    resolve --> display["render the merge list<br/>print_entry_line() / print_tree() / print_json()<br/>+ counters, USE strings, blockers"]
    display --> pretendq{"--pretend / -p ?"}
    pretendq -->|yes| exit0["exit 0 — nothing touched"]:::done
    pretendq -->|no| exec["execution branch<br/>(buildpkgonly / getbinpkg / source merge)"]:::branch

    classDef entry fill:#1f6f43,color:#fff
    classDef done fill:#4a4a4a,color:#fff
    classDef branch fill:#8a5a1a,color:#fff
```

The execution branch (`if !pretend`, `pretend.rs:7127`) picks exactly one
of:

- `emerge_build::run_buildpkgonly` — `--buildpkgonly` / `-B`
- `emerge_getbinpkg::run_merge_plan` — `--getbinpkg` / `--getbinpkgonly`
- `emerge_build::run_source_merge` — plain `emerge <atom>`
