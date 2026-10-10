# Killing a Program and All Its Descendants (Including Podman Containers) Without `pkill`

## Goal

Run a program (e.g., a bash shell) so that, at any later time, you can terminate **it and every descendant** — child processes, granddaemons, *and detached podman containers it launched* — with one action, without relying on `pkill` name matching.

## Why process-tree approaches are not enough

- **PID namespaces / `unshare --pid`**: you cannot retroactively unshare a running process; the namespace is fixed at fork time. You must launch the program into it from the start:
  ```bash
  sudo unshare --pid --fork --mount-proc yourprogram
  ```
  Killing the namespace init (PID 1 of the new namespace) makes the kernel SIGKILL every process left in the namespace. But this \*\*does not work with podman containers\*\*: \`podman run -d\` double-forks through \`conmon\`, which daemonizes and detaches from your process tree; the container also runs in its own PID namespace. Detached containers are \*not\* descendants of the shell.
- **Process groups (`setsid` + `kill -TERM -- -PGID`)**: children can escape by calling `setsid()` themselves, and detached containers were never in the group.

## The key fact: containers escape the process tree, not the cgroup hierarchy

Podman creates each container's cgroup **under the cgroup of the process that invoked `podman`**. So if the shell lives in a dedicated cgroup, every container it launches is nested below it. Cgroups are therefore the reliable kill boundary.

## Recipe A — rootful, raw cgroups v2

Requirements: cgroup v2, kernel ≥ 5.14 (`cgroup.kill` file), root.

```bash
# once, as root
sudo mkdir /sys/fs/cgroup/myshell
echo $$ | sudo tee /sys/fs/cgroup/myshell/cgroup.procs   # move this shell in

# work normally; container cgroups appear under /sys/fs/cgroup/myshell/
podman run -d ...

# hard-kill the entire subtree: this shell, children, conmon, all containers
echo 1 | sudo tee /sys/fs/cgroup/myshell/cgroup.kill
```

Caveats:

- `cgroup.kill` is synchronous, uncatchable — effectively SIGKILL for every PID in the subtree. Containers get no chance to shut down cleanly.
- Container storage entries survive the kill: run `podman rm -f` / `podman system prune` afterwards.

## Recipe B — rootless, delegated systemd scope (preferred when possible)

Rootless podman cannot create sub-cgroups unless systemd **delegates** a cgroup subtree to your user. Use a delegated scope:

```bash
systemd-run --user --scope -p Delegate=yes bash
# inside: podman run -d ...   (containers nest inside the scope's cgroup)

# from outside: SIGTERM to everything in the scope, then SIGKILL to survivors
systemctl --user stop run-<something>.scope
```

Advantages over raw `cgroup.kill`: graceful TERM→KILL shutdown sequence, no root needed, works for rootless podman.

### Troubleshooting: `Failed to connect to user scope bus: $DBUS_SESSION_BUS_ADDRESS and $XDG_RUNTIME_DIR not defined`

The user manager (`user@<uid>.service`) exists but your shell lacks the env vars (typical after `su`, `sudo -s`, or a PAM-less SSH path). Diagnose with:

```bash
ls -ld /run/user/$(id -u)          # runtime dir present?
systemctl status user@$(id -u)     # user manager running?
```

If the runtime dir exists, just reconnect to your bus:

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
```

If the user manager is not running:

```bash
sudo loginctl enable-linger $USER   # start at boot, keep running without sessions
sudo systemctl start user@$(id -u)
```

Fallback without fixing the session: `systemd-run --machine=$USER@.host --user ...` (routes through a fresh SSH login so PAM sets up the session).

## Recipe C — rootless podman without systemd

If the box has no user systemd instances at all, rootless podman can run with `podman run --cgroups=disabled`: containers get no cgroup of their own, but their processes still live in whatever cgroup your shell is in. If the shell is in a cgroup you control, `cgroup.kill` on that cgroup still takes them down.

## Summary


| Approach                                         | Reaches detached podman containers? | Notes                                  |
| ------------------------------------------------ | ----------------------------------- | -------------------------------------- |
| PID namespace (`unshare --pid --fork`)           | no                                  | conmon detaches from the process tree  |
| `setsid` + kill process group                    | no                                  | children/containers escape the group   |
| Raw cgroup v2 `cgroup.kill`                      | yes                                 | rootful, kernel ≥ 5.14, hard kill only |
| Delegated `systemd-run --user --scope`           | yes                                 | rootless-friendly, graceful TERM→KILL  |
| `podman --cgroups=disabled` + parent cgroup kill | yes                                 | fallback when no user systemd          |

