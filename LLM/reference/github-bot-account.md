# The `Im-IA` GitHub account: how Claude works on portuale from a machine

Written 2026-11-27 for the Claude that runs inside the `portuale` container
(`ssh vivo@portuale`). The host Claude was set up the same way the same day;
this file reproduces that set-up so the two stay identical.

## What this account is

`Im-IA` (<https://github.com/Im-IA>) is a separate GitHub account that the owner
(`vivo75`) created for Claude. It is a collaborator on `vivo75/portuale` and
`vivo75/pmtest` with `pull`, `push` and `triage`, but **not** `admin` or
`maintain`. It is a machine account: the owner is responsible for what it does.

Everything below exists so that Claude acts as `Im-IA` and **never** as
`vivo75`. The owner's own credentials are on the same machines; do not use them.

## Rules

- **Never use `vivo75`'s credentials.** `gh` in `~/.config/gh` is logged in as
  the owner (plus expired work tokens). Do not run `gh` without
  `GH_CONFIG_DIR=$HOME/.config/gh-im-ia`, and never `gh auth switch`,
  `gh auth logout`, `gh auth refresh` on the default config.
- **Push only through the `Im-IA-github` remote**, never through `origin`
  (`origin` uses the owner's key). `portuale` first or `pmtest` first follows
  the pairing rule in [`AGENTS.md`](../../AGENTS.md) "Two repos, one project":
  pmtest first, then portuale.
- **Commit and push only when explicitly asked** (`AGENTS.md` rule 9). Having
  access changes nothing about that.
- **Identity is repo-local.** Never write `user.name` / `user.email` with
  `--global`: the global identity is the owner's.
- **The private key is not copied anywhere else, and no token is ever written
  into a document, a commit message, a log or a PR.**
- If something here does not match reality, say so to the owner; do not
  improvise a workaround with the owner's credentials.

## The set-up

| Piece | Value |
|---|---|
| SSH key | `~/.ssh/id_fs-test` (the same key on the host and in the container) |
| SSH host alias | `Im-IA-github` → `github.com`, `IdentityFile ~/.ssh/id_fs-test` |
| Remote (both repos) | `Im-IA-github` = `git@Im-IA-github:vivo75/<repo>.git` |
| Git identity (both repos, local) | `Im-IA <vivo75+AvianIntelligence@gmail.com>` |
| `gh` config dir | `~/.config/gh-im-ia` (token stored in a file there, not in the keyring) |

The repos are `~/repo/PORTUALE/portuale` and `~/repo/PORTUALE/pmtest`.

## State of the container when this was written (2026-11-27)

Checked read-only from the host:

- done: `~/.ssh/id_fs-test` is present and its public half is byte-identical to
  the host's (same sha256).
- done: the `Im-IA-github` host alias is in `~/.ssh/config`.
- done: the `Im-IA-github` remote exists in both repos, next to `origin`.
- **missing:** no repo-local `user.name` / `user.email` (the global identity,
  the owner's, would be used, so commits would be attributed to `vivo75`).
- **missing:** `gh` is not logged in as `Im-IA`; `~/.config/gh-im-ia` does not
  exist.
- was broken, fixed by the owner the same day: `~/.ssh/config` had a stray
  line consisting of a single `s` after `Host git.f1r.eu`, which made OpenSSH
  stop with `line 2: no argument after keyword "s"` and fail **every** `ssh`
  and `git` call over SSH. If that error ever comes back, the file is the
  owner's: report it, do not edit it.
- done after the fix: `ssh -T git@Im-IA-github` answers `Hi Im-IA!` and
  `git ls-remote --heads Im-IA-github` works in both repos.

Re-check each item before relying on this list; it is a snapshot.

## Steps

### 1. Check SSH reaches GitHub as `Im-IA`

```
ssh -o BatchMode=yes -T git@Im-IA-github
```

Expected: `Hi Im-IA! You've successfully authenticated, but GitHub does not
provide shell access.` Note the `git@`: without it ssh uses the local user name
and prints a misleading `Permission denied (publickey)`.

Then `git ls-remote --heads Im-IA-github` in each repo must list branches.

If the config error above shows up, stop and ask the owner to fix the stray
line first.

### 2. Set the identity in each repo, locally

```
for d in ~/repo/PORTUALE/portuale ~/repo/PORTUALE/pmtest; do
  git -C "$d" config --local user.name  "Im-IA"
  git -C "$d" config --local user.email "vivo75+AvianIntelligence@gmail.com"
done
```

That address must be a verified email of the `Im-IA` account for GitHub to link
commits to the profile; if it is not, pushes still work, commits just show no
avatar.

### 3. `gh` login: a step for the owner, not for Claude

Device-flow login needs a browser signed in as `Im-IA`. The owner types, in the
Claude prompt (the `!` prefix runs it in the session):

```
! GH_CONFIG_DIR=$HOME/.config/gh-im-ia gh auth login -h github.com -p ssh -s repo,workflow,read:org --insecure-storage -w
```

The container has no browser, so the owner opens the printed URL on their own
machine and types the one-time code there, signed in as `Im-IA`. If `gh` offers
to upload an SSH key, answer no: `id_fs-test` is already registered, and asking
for `admin:public_key` is not needed. (On the host the token ended up with that
extra scope anyway; leaving it out is preferable.)

`--insecure-storage` keeps the token in `~/.config/gh-im-ia/hosts.yml`, so it
does not mix with the owner's keyring entry. Do not read that file aloud and do
not copy it.

### 4. Verify

```
export GH_CONFIG_DIR=$HOME/.config/gh-im-ia
gh auth status                 # Logged in to github.com account Im-IA
gh api user --jq .login        # Im-IA
for r in portuale pmtest; do gh api repos/vivo75/$r --jq .permissions; done
```

Expected permissions: `push` and `pull` and `triage` true, `admin` and
`maintain` false. If `gh api user` prints `vivo75`, the config dir is wrong:
stop, do nothing else, tell the owner.

## Using it

- Prefix every `gh` call with `GH_CONFIG_DIR=$HOME/.config/gh-im-ia`.
- Branches are opened in both repos under the same name (`AGENTS.md`). The
  container's two repos are currently on the same branch; check before working.
- Pull requests are opened as `Im-IA`. The attribution line of a PR body and the
  `Co-Authored-By:` trailer of a commit name the actual model, as `AGENTS.md`
  rule 9 says.
- With only `push` and `triage`, branch protection and repository settings are
  the owner's to change; do not try to work around a protected `main`.
