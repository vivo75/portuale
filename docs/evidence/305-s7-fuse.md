# #305 (feat#157) — S7.4 FUSE view with outside tools: NOT RUN (blocked)

**Status: blocked, to be run by the owner on a FUSE-capable host.**
The host the plan was executed on (2026-10-05) runs inside a container
without `/dev/fuse` (`fusermount3` exists, the device does not), so
nothing can be mounted there. What S7.2/S7.3 could check without a
mount is covered by the unit tests in `rust/portuale/src/vdb_view.rs`
(tree, attributes and bytes against the real files tree on files /
sqlite / redb, metadata stamp rewrite accepted by real's reader rule,
offset reads, generation pinning, inode stability, EROFS). The fuser
adapter (`vdb_fuse.rs`) and its fork/signal code are only compiled.

## Commands for the owner

```sh
B=portuale                                   # a build with default features
$B vdb convert --from files:/ --to sqlite:/tmp/vdb.sqlite
mkdir -p /mnt/vdb /mnt/pass
$B vdb mount --allow-other sqlite:/tmp/vdb.sqlite /mnt/vdb     # add -f to stay in the foreground

# outside tools through the view vs the original (point each tool's VDB/ROOT at the mount
# the way the tool allows: a bind mount over a chroot's /var/db/pkg is the most faithful)
qlist -I | sort > /tmp/qlist.orig
# (run the same with the view bind-mounted over a chroot's var/db/pkg) > /tmp/qlist.view
eix --installed ... ; equery list '*' ; emerge -p @world        # same orig/view pairs, then diff

touch /mnt/vdb/x            # expect: Read-only file system (EROFS)
fusermount3 -u /mnt/vdb

$B vdb mount files:/ /mnt/pass && diff -r /var/db/pkg /mnt/pass && fusermount3 -u /mnt/pass

$B vdb convert --from files:/ --to redb:/tmp/vdb.redb
$B vdb mount redb:/tmp/vdb.redb /mnt/vdb
mrg --pretend --vdb-backend=redb --vdb-path=/tmp/vdb.redb @world   # expect: Busy (offline view)
fusermount3 -u /mnt/vdb
```

Record the outputs and diffs here when run.
