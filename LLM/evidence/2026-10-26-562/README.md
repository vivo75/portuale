# Live Portage oracle: the `updates` record commits pre-resolve, even on failure (#562)

Host `emerge` 3.0.83 (this machine, uid 1000), scratch configroot
pointing at the pmtest fixture `repo` only, scratch ROOT, run 2026-10-26:

```
ROOT=/tmp/live562/root PORTAGE_CONFIGROOT=/tmp/live562/cfg emerge \
  --pretend --oneshot --color=n dev-libs/schedok
```

`make.conf` holds only `FEATURES="unprivileged -sandbox
-usersandbox"` (`unprivileged` earns `secpass = 2` for a non-root user,
so `_global_updates` is not gated out; `data.py`).

Result: rc 1 -- the fixture `default` profile is invalid to real
Portage (`pretend1.err`: "Your current profile is invalid"), so the
resolve never ran. Nevertheless `ROOT/var/cache/edb/mtimedb`
(`mtimedb` in this dir) was committed with an `updates` section
containing the fixture key
`/home/vivo/repo/PORTUALE/pmtest/fixtures/repo/profiles/updates/2Q-2024`
(record-key shape `<repo-location>/profiles/updates/<file>`, exactly
what portuale records), alongside the host gentoo repo's own entries
(`/.gentoo/...` -- the host tree leaks in through the global
repos.conf, the S0 report's concern #2).

`pretend1.out` head shows the `Performing Global Updates` block with
the fixture file yielding `..` (two valid directives parsed) -- the
block portuale deliberately stays cut on ("modulo Global Updates").

What this proves (grounds the #179/`ask_declined` narrowings, which no
longer assert byte-identical/absent mtimedb after `--pretend`):

- `run_action` (`lib/_emerge/actions.py:3941-3964`) has no `--pretend`
  exemption: the `updates` record commits pre-resolve, on `--pretend`
  and on runs whose resolve never happens.
- The force/record key is the updates-file path string.
