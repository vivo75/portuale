# Remote install: worked example with `app-portage/eix`

A start-to-end recipe for installing `app-portage/eix` on a **client**
container from a **server** container running portuale (`mrg --remote-*`),
then building the installed-package database (VDB) in each of the three
backends: `files`, `sqlite`, `redb`.

Design and option reference: [`remote-merge.md`](remote-merge.md) (remote
merge), [`feat-157-authoritative-vdb-database.md`](feat-157-authoritative-vdb-database.md)
(backends). This page is runnable examples only.

**Status of each part** (nothing here replaces the L1/L4 beds):

| Part | State |
|---|---|
| §6 conversion (`vdb convert` / `verify` / `status`) | run live 2026-10-07 in `localhost/test-portuale:latest` against its 320-entry VDB: both conversions rc 0, all three `verify` pairs `equal: 320 entries compared` |
| §1–§5, §7 (containers, eix build, remote merges) | **written from the source, not yet run end to end** |

## 0. What the backends mean in a remote run

- The client's VDB is always a **files** tree. `mrg` refuses
  `--vdb-backend`/`--vdb-path` together with the default
  `--remote-vdb=client:…` (`mrg.rs`, `setup_vdb`): exit 2.
- `sqlite` and `redb` are selectable on the merge only with
  `--remote-vdb=server:<path>`, the stateless-client mode (files merge, no
  client VDB entry, no old hooks, fail-closed collisions).
- To have sqlite/redb hold the **real** eix VDB, merge normally (files),
  copy the client's VDB to the server and convert it (§6).
- Open point: `--remote-vdb=server:<path>` feeds a pre-ship ownership
  shadow that reads `<path>/<cat>/<pf>/CONTENTS` as a files tree
  (`remote.rs`, `VdbShadow::load`). Whether it consults a sqlite/redb file
  is not established; §5 observes it.

## 1. Host setup

```bash
cd portuale/rust && cargo build --release      # default features: vdb-sqlite, vdb-redb, vdb-fuse
BIN=$PWD/target/release                        # portuale + mrg/emerge/ebuild symlinks
K=/var/tmp/rm-keys; mkdir -p $K
ssh-keygen -t ed25519 -N '' -q -f $K/id && cp $K/id.pub $K/authorized_keys
IMG=localhost/test-portuale:latest             # any image with sshd, ssh, bash >= 5.3, tar works
podman network create rmnet
podman volume create rm-pkgs                   # PKGDIR, survives client resets
```

The image's entrypoint is a custom `init` that exits at once: always pass
`--entrypoint /bin/bash`.

## 2. Client container

Only sshd runs. No portuale on `PATH`, no binhost. The ssh workarounds are
the ones `pmtest/differential-test-bed/layers/l31/consume-remote.sh` uses
(the image ships an `ssh_config.d` symlink with mode 0777 that makes every
ssh call abort).

```bash
podman rm -f client 2>/dev/null
podman run -d --name client --network rmnet --hostname client --entrypoint /bin/bash \
  -v $K:/keys:ro $IMG -c '
  mkdir -p /var/empty /run/sshd /root/.ssh
  chown root:root /var/empty /run/sshd; chmod 755 /var/empty /run/sshd
  rm -f /etc/ssh/ssh_config.d/20-systemd-ssh-proxy.conf
  chown -R root:root /etc/ssh; chmod 644 /etc/ssh/ssh_config
  ssh-keygen -A -q
  chown -R root:root /root     # the image ships /root as bin:bin; sshd StrictModes refuses key auth otherwise
  cp /keys/authorized_keys /root/.ssh/; chmod 700 /root/.ssh; chmod 600 /root/.ssh/authorized_keys
  printf "PasswordAuthentication no\nPermitRootLogin prohibit-password\nUsePAM no\n" >> /etc/ssh/sshd_config
  exec /usr/sbin/sshd -D -e'
```

### 2.1 Which ssh user: root with a key, not a sudo user

`mrg` has **no privilege escalation** in v1: it runs the generated driver
directly as the ssh login user, which "must own the target ROOT"
(`remote-merge.md` §1, §4; `--remote-become` is a named future slice and
does not exist in `remote.rs`). A `portuale` user with passwordless `sudo`
would therefore change nothing, and the image has no `sudo` anyway (only
`su`, which cannot run unattended).

For a merge into `/` the login user must be root. The block above already
configures that: `PermitRootLogin prohibit-password` allows root over a key
only, `PasswordAuthentication no` closes passwords, and the server's key is
the only entry in `/root/.ssh/authorized_keys`. Nothing else needs
configuring.

To test a non-root login (**not run**), make the user own a target root
instead of `/`. Add this to the client's startup script, then pass
`--remote-user=portuale --remote-root=/home/portuale/root`:

```bash
useradd -m portuale && mkdir -p /home/portuale/.ssh /home/portuale/root/var/db/pkg
cp /keys/authorized_keys /home/portuale/.ssh/
chown -R portuale: /home/portuale; chmod 700 /home/portuale/.ssh; chmod 600 /home/portuale/.ssh/authorized_keys
```

Files that need root ownership will not be chowned, so expect ownership
differences against a root merge. That is the point of the "client must
own ROOT" rule, not a bug. The §5.1 verification commands then look under
`/home/portuale/root/var/db/pkg`.

`/root` must be owned by root: this image ships it as `bin:bin`, and
sshd's default `StrictModes yes` then rejects the key ("Authentication
refused: bad ownership or modes" in the sshd log, visible because the
daemon runs with `-e`). The differential bed's `consume-remote.sh` avoids the
chown with `StrictModes no` in `sshd_config`; either works. The same applies
to the non-root user in §2.1: its home and `~/.ssh` must belong to it.

Re-run this block to reset the client between backend runs; recreating it
is cleaner than unmerging.

## 3. Server container

```bash
podman rm -f server 2>/dev/null
podman run -d --name server --network rmnet --entrypoint /bin/bash \
  -v $BIN:/usr/local/bin:ro \
  -v $K:/keys:ro -v rm-pkgs:/var/cache/binpkgs $IMG -c '
  mkdir -p /var/empty /run/sshd; chown root:root /var/empty; chmod 755 /var/empty
  rm -f /etc/ssh/ssh_config.d/20-systemd-ssh-proxy.conf
  chown -R root:root /etc/ssh; chmod 644 /etc/ssh/ssh_config
  rm -f /etc/portage/binrepos.conf/gentoo.conf    # resolve from the local PKGDIR only
  exec sleep infinity'
podman exec server bash -c 'cp /keys/id /root/id && chmod 600 /root/id'

# gate: ssh works and the client meets the tool floor (bash >= 5.3, tar)
podman exec server ssh -i /root/id -o StrictHostKeyChecking=accept-new root@client \
  'bash --version | head -1; tar --version | head -1'
```

The binary carries its vendored phase runtime (`bin/`), so the server needs no
checkout: only the executable is mounted. To use a different copy of `bin/`,
set `PORTUALE_BIN_DIR` (a set but wrong value is an error, never a silent
fallback). The server also ships that `bin/` to the client with each unit, so
the client needs no checkout either (backlog #322).

## 4. Build the eix gpkg on the server (once)

```bash
podman exec server bash -c '
  export BINPKG_FORMAT=gpkg FEATURES="buildpkg -sign" PKGDIR=/var/cache/binpkgs
  emerge -1v --buildpkg=y app-portage/eix
  ls /var/cache/binpkgs/app-portage/eix/*.gpkg.tar /var/cache/binpkgs/Packages'

# preview: --pretend stays local and ignores --remote-*
podman exec server mrg -pv --getbinpkgonly app-portage/eix
```

`xpak` (`.tbz2`) is not a supported `mrg` remote format; `BINPKG_FORMAT=gpkg`
is required. Dependencies already installed on the client resolve as
`AlreadyInstalled` (no-op).

## 5. Install on the client, per backend

### 5.1 `files` (default: the client holds its own VDB)

```bash
podman exec server mrg -v --getbinpkgonly \
  --remote-hostname=client --remote-user=root --remote-key-file=/root/id \
  --remote-vdb=client:/var/db/pkg app-portage/eix
```

Expect `>>> Remote …` lines and a `>>> Remote summary` line. Verify:

```bash
podman exec client bash -c '
  ls -d /var/db/pkg/app-portage/eix-*; head -3 /var/db/pkg/app-portage/eix-*/CONTENTS
  eix --version; eix-update >/dev/null; eix -I eix'
podman exec server bash -c 'tail -3 /var/cache/binpkgs/remote-ledger/* 2>/dev/null'  # server ledger
```

### 5.2 `sqlite` and `redb` (stateless client, database on the server)

Recreate the client (§2) before each, then:

```bash
for B in sqlite redb; do
  podman exec server mkdir -p /srv/vdb-$B
  podman exec server mrg -v --getbinpkgonly \
    --remote-hostname=client --remote-user=root --remote-key-file=/root/id \
    --remote-vdb=server:/srv/vdb-$B --vdb-backend=$B --vdb-path=/srv/vdb-$B/vdb.$B \
    app-portage/eix
  podman exec client bash -c 'command -v eix; ls /var/db/pkg/app-portage 2>&1'
done
```

Expected from the source: eix's files land on the client (`eix --version`
works), there is **no** client VDB entry, and `redb` starts the parent IPC
pipe (`PORTUALE_VDB_IPC`) so its single-process lock holds. Negative test:
`--vdb-backend=sqlite` with `--remote-vdb=client:/var/db/pkg` must exit 2
with "cannot be combined with a client: VDB".

## 6. Create the databases with the conversion tool

After §5.1 the client holds the real eix VDB as a files tree. Pull it to the
server and convert it into both databases. `files:ROOT` names a **root**: the
VDB directory under it plus the stores `var/lib/portage/{world,world_sets,
preserved_libs_registry}` and `var/cache/edb/counter`.

```bash
# 1. pull the client's VDB + stores into a root laid out like a client root
podman exec client bash -c 'cd / && tar -cf - var/db/pkg \
  $(ls -d var/lib/portage/world var/lib/portage/world_sets \
          var/lib/portage/preserved_libs_registry var/cache/edb/counter 2>/dev/null)' \
  | podman exec -i server bash -c 'rm -rf /srv/cv && mkdir -p /srv/cv && tar -C /srv/cv -xf -'

# 2. convert the files tree into each database (the file is created when missing)
podman exec server bash -c '
  portuale vdb convert --from files:/srv/cv --to sqlite:/srv/cv.sqlite
  portuale vdb convert --from files:/srv/cv --to redb:/srv/cv.redb'
```

Each prints `converted N entries (files -> <kind>); world …; counter …`.
Counters are preserved, never renumbered. Re-converting into an existing
file needs `--force`.

### 6.1 Verify

```bash
podman exec server bash -c '
  portuale vdb verify files:/srv/cv      sqlite:/srv/cv.sqlite   # equal: N entries compared
  portuale vdb verify files:/srv/cv      redb:/srv/cv.redb
  portuale vdb verify sqlite:/srv/cv.sqlite redb:/srv/cv.redb
  portuale vdb status sqlite:/srv/cv.sqlite                      # pending: none
  portuale vdb status redb:/srv/cv.redb
  ls -la /srv/cv.sqlite /srv/cv.redb'
```

Exit codes: 0 equal, 1 differences found, 2 usage or I/O error. A redb file
held open by another process fails with "database is already open" (exit 2).
Do not run two redb commands on the same file at once.

### 6.2 Check eix is in the databases

```bash
podman exec server bash -c '
  portuale vdb rebuild-index sqlite:/srv/cv.sqlite       # recompute owner/dep_atom/needed from entry files
  portuale vdb status redb:/srv/cv.redb | grep -E "installed|counter"'
```

To read a database with eix/qlist through the FUSE view, start the server
container with `--device /dev/fuse --cap-add SYS_ADMIN`, then:

```bash
podman exec server bash -c '
  mkdir -p /mnt/vdb && portuale vdb mount sqlite:/srv/cv.sqlite /mnt/vdb
  ls /mnt/vdb | head; ls -d /mnt/vdb/app-portage/eix-*; umount /mnt/vdb'
```

Use `--rw [--root ROOT]` for the read-write mount. The mount commands are
unverified in this example; see
[`evidence/305-s7-fuse.md`](evidence/305-s7-fuse.md) and
[`evidence/317-s5-host.md`](evidence/317-s5-host.md) for what was checked.

### 6.3 Already-installed VDB as the source

The same conversion works on any root, for instance the server's own:

```bash
podman exec server portuale vdb convert --from files:/ --to sqlite:/tmp/host.sqlite
```

(`files:/` and `files:/var/db/pkg` mean the same root.)

## 7. Failure and gate checks

| Check | How | Expect |
|---|---|---|
| Dead host | `--remote-hostname=nonexistent` | ssh rc 255 mapped to "client unreachable"; nothing written |
| TOFU | first connect to a fresh client | fingerprint printed; a changed key (after recreating the client) aborts. Clear the server's `known_hosts` or pass `--remote-strict-host-key-checking=no` between resets |
| Clock skew | `podman exec client date -s '+20 min'` (client needs `--cap-add SYS_TIME`) | aborts at preflight; `--remote-max-clock-skew=0` disables |
| Not binary-only | drop `--getbinpkgonly` | exit 2 |
| Backend vs `client:` VDB | `--vdb-backend=sqlite` with default placement | exit 2 |

## 8. Known caveats

- pmtest's L4 capture `l31-s0` found `mrg` remote dropping setuid/setgid/
  sticky bits. eix ships none, so it is unaffected.
- A client phase's `portageq-wrapper` shim execs `portuale` from the
  client's `PATH` over ssh. If eix's postinst needs it, mount `$BIN`
  read-only into the client, or set `PORTUALE_BIN` there.
- `--remote-jobs` is sequential only.

## 9. Clean up

```bash
podman rm -f client server; podman network rm rmnet; podman volume rm rm-pkgs; rm -rf $K
```
