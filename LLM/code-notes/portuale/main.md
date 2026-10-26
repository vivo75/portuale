# crates/portuale/src/main.rs
Multicall binary dispatch (`emerge`, `ebuild`, `mrg`, `vdb`, `portageq`).

## module
- Proves the emerge/ebuild dispatch mechanism described in LLM/agent-context.md ("emerge/ebuild binary shape"): a single static binary behaving differently depending on how it is invoked, busybox-style. The install creates `emerge` / `ebuild` symlinks (or hardlinks) pointing at it; argv[0] tells it which applet to run.
- Both applets do live work: `emerge` resolves dependencies and builds / merges / unmerges packages (see pretend.rs and its `emerge_*` siblings), `ebuild` runs live phase chains and the live merge / unmerge / package steps (see ebuild.rs). Both recognize their whole Portage CLI surface by name (see emerge_options.rs / ebuild_options.rs) even where a given flag or action isn't implemented, reporting which one it is rather than a generic error.

## fn select_vdb_for
- #318: the backend `PORTUALE_VDB_BACKEND` selects (environment, then make.conf), opened and registered before the applet runs, the same rule `mrg` applies without its flags. `None` keeps the run going on the files tree; `Err` is the exit status to stop with.

## fn run_emerge
- `docs/history/second_python_copy_removal.md` §4: the L0 bed and the contract suite ask for the count of dependency tokens the resolver dropped as unparseable (via `PORTUALE_REPORT_UNPARSED_DEP_TOKENS`), and assert it is zero.

## fn run_ebuild
- Read-write even for a build-only command because redb allows one process per file: its phases' `has_version` reach a redb file only through the parent pipe a read-write open starts.

## fn exit_quietly_on_broken_pipe
- With `panic = "abort"` the SIGPIPE-ignored `println!` panic ("failed printing to stdout: Broken pipe") would be a core dump; the hook exits 141 instead, silently.
- The global-updates block writes through its own `expect`s (`output.rs::emit`), so a closed pipe there panics with "global updates stdout write/flush: Broken pipe" instead of the `println!` text; the hook converts that shape too, since the early actions print the block before any `println!` runs.

## fn main
- The helper entry runs first (backlog #326 D7), from `args_os()`: no config, no `bin_dir()`, no extraction, no atexit.
- The binary's path is pinned before any phase can run (backlog #331), so a file replaced mid-run never surfaces as `<path> (deleted)`.
- The aux-metadata provider registration is item C2: `portage-repo` itself carries no phase runner; the cache-miss metadata provider `portage-repo` asks from `repo_aux_metadata` (the ebuild fallback Portage's `porttree.py` runs when `metadata/md5-cache` lacks the entry); the binary that owns live ebuild-phase execution registers it. Unregistered (unit tests), a miss keeps the old read error.
- Dual invocation matches busybox's own style (argv[0] symlink, or explicit first argument).

## tests: a_closed_stdout_pipe_exits_141_without_a_panic
- Backlog #323: output into a closed pipe is a quiet 141, not a panic message and an abort (SIGABRT, 134).
