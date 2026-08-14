# Local-first distfile mirror (nginx) — container test bed / other hosts

A caching HTTP mirror that serves this host's existing flat distfiles
store (`/.gentoo/cache/distfiles`, the target of `/var/cache/distfiles`)
**first**, and only reaches upstream for files it doesn't already have.
It exists to stop the pmtest differential test bed and other hosts from
re-downloading distfiles this container already holds (~80 GB / ~6.9k
files as of 2026-10-08).

Config-only: nothing in the `portuale` or `pmtest` source trees is
touched. This file lives in `LLM/reference/` (moved from the untracked
`helpers/`, 2026-10-10).

## Why not squid

Squid's disk store is its own binary format; it cannot serve an existing
plain directory of tarballs. Pointing squid at
`/.gentoo/cache/distfiles` is not possible — it would build a *second*
cache and only help from the second request onward, never reusing the
~80 GB already on disk. "Try the local files first" needs a server that
reads a loose directory with an upstream fallback: nginx.

## The one non-obvious bit: the mirror layout is *hashed*

`distfiles.gentoo.org` serves `[structure] 0=filename-hash BLAKE2B 8`,
i.e. a file lives at

```
/distfiles/<first 2 hex of BLAKE2B-512(filename)>/<filename>
```

Flat `distfiles.gentoo.org/distfiles/<filename>` URLs **404**. Our local
store is flat (bare filenames), so:

- the mirror **advertises the same hashed layout** in `layout.conf`, so a
  real client (portage/portuale) computes the hash and requests the
  hashed URL — which is exactly what upstream serves;
- nginx **strips** the 2-hex prefix to find the bare filename in the flat
  store (a hit → served locally, zero upstream traffic);
- on a miss, nginx proxies the *original* hashed URI to upstream.

nginx cannot compute BLAKE2B, which is why the hashed layout has to come
from the client rather than being synthesised server-side.

## Files

- `/etc/nginx/conf.d/distfiles-mirror.conf` — the vhost (the file below).
- `/etc/nginx/nginx.conf` — one added line inside `http {}`:
  `include conf.d/*.conf;` (backup: `nginx.conf.bak-portuale-mirror`).
- `/var/cache/portuale-distfiles-mirror/` — nginx's own atomic proxy
  cache for upstream misses (owner `nginx:nginx`).
  Deliberately **not** under `/var/cache/nginx`, because
  `/usr/lib/tmpfiles.d/nginx-tmp.conf` has `e! /var/cache/nginx/` which
  empties that directory at boot.

The shared store is **never written to** by nginx: fills land in the
proxy cache, not in `/.gentoo/cache/distfiles`. This is intentional — nginx
`proxy_store` into the live store is not atomic and a truncated fill
would poison a directory that local portage/portuale read directly.
Clients verify every distfile against the `Manifest`, so a partial nginx
cache entry can never be mistaken for a good file.

## The vhost

```nginx
proxy_cache_path /var/cache/portuale-distfiles-mirror
                 levels=1:2
                 keys_zone=distfiles_mirror:64m
                 max_size=200g
                 inactive=90d
                 use_temp_path=off;

server {
    listen 8080;
    server_name _;

    allow 127.0.0.1;
    allow ::1;
    allow 10.0.0.0/8;
    allow 169.254.0.0/16;
    deny all;

    location = /distfiles/layout.conf {
        default_type text/plain;
        add_header Cache-Control "public, max-age=86400" always;
        return 200 "[structure]\n0=filename-hash BLAKE2B 8\n";
    }

    location ~ "^/distfiles/[0-9a-fA-F]{2}/(.+)$" {
        root /.gentoo/cache;
        try_files /distfiles/$1 @upstream;
        add_header X-Distfiles-Source "local" always;
    }

    location /distfiles/ {
        root /.gentoo/cache;
        try_files $uri @upstream;
        add_header X-Distfiles-Source "local-flat" always;
    }

    location @upstream {
        internal;

        # No working IPv6 here: resolve IPv4 only (nginx >= 1.27.4).
        resolver 185.12.64.1 185.12.64.2 74.82.42.42 ipv6=off valid=300s;
        set $distfiles_host distfiles.gentoo.org;
        proxy_pass https://$distfiles_host$request_uri;
        proxy_ssl_server_name on;
        proxy_ssl_name distfiles.gentoo.org;

        proxy_set_header Host distfiles.gentoo.org;
        proxy_set_header User-Agent "portuale-local-mirror";

        proxy_cache distfiles_mirror;
        proxy_cache_key "$uri";
        proxy_cache_valid 200 301 302 30d;
        proxy_cache_lock on;
        proxy_cache_use_stale error timeout updating
                              http_500 http_502 http_503 http_504;

        proxy_max_temp_file_size 0;

        add_header X-Distfiles-Source "upstream" always;
        add_header X-Cache-Status $upstream_cache_status always;
    }
}
```

## Setup / lifecycle

```sh
sudo mkdir -p /var/cache/portuale-distfiles-mirror
sudo chown nginx:nginx /var/cache/portuale-distfiles-mirror
sudo cp /tmp/opencode/distfiles-mirror.conf /etc/nginx/conf.d/distfiles-mirror.conf
sudo nginx -t && sudo systemctl restart nginx   # 'restart' required if the
                                                # cache path ever changes;
                                                # nginx refuses a 'reload' that
                                                # moves an existing cache zone
```

`nginx.service` is already `enabled`, so it comes up on boot.

## Client wiring (no code change)

Point the client's mirror at `http://10.48.0.229:8080`. Both real
`emerge` and portuale honour `GENTOO_MIRRORS`:

```sh
# ephemeral / per-run
sudo env GENTOO_MIRRORS="http://10.48.0.229:8080" emerge ...
# or in a test-bed container's make.conf:
#   GENTOO_MIRRORS="http://10.48.0.229:8080 http://distfiles.gentoo.org"
```

A `GENTOO_MIRRORS` entry is tried as a public-mirror candidate, and a
miss on it (a genuine 404 for a file not upstream either) falls through
to any other entry. `10.0.0.0/8` is allowed, which covers the LXD bridge
peers; add ranges to the `allow` block if the test bed uses another net.

Optional local shortcut (clients that share this filesystem): add the
store as a read-only distdir source, which digest-verifies and symlinks
into `DISTDIR` before any download:
`PORTAGE_RO_DISTDIRS=/.gentoo/cache/distfiles`.

## Verification (2026-10-08, this host)

```
# layout negotiated by a real client (Portage):
10.48.0.229 "GET /distfiles/layout.conf"               200 38   Portage/...
10.48.0.229 "GET /distfiles/80/which-2.23.tar.gz"      200 201930  # local store hit
10.48.0.229 "GET /distfiles/cb/nano-9.0.tar.xz"        200 1743088 # fill from upstream

# which-2.23.tar.gz (present in store) -> served locally, byte-identical
# nano-9.0.tar.xz   (absent)           -> MISS then HIT, sha == upstream
# range/resume: 206 Partial Content for both local and cached files
# real fetch: ebuild ... fetch -> "* nano-9.0.tar.xz BLAKE2B SHA512 size ... [ ok ]"
#                                 "* which-2.23.tar.gz BLAKE2B SHA512 size ... [ ok ]"
# store untouched: /.gentoo/cache/distfiles/nano-9.0.tar.xz still absent
# error log clean (no IPv6 upstream errors after the ipv6=off change)
```

Handy probes:

```sh
h=$(printf '%s' FILENAME | b2sum -l 512 | cut -c1-2)
curl -sD- -o/dev/null "http://10.48.0.229:8080/distfiles/$h/FILENAME" | grep -Ei 'HTTP/|X-Distfiles|X-Cache-Status'
```

`X-Distfiles-Source: local` = served from the store (no upstream).
`X-Distfiles-Source: upstream` + `X-Cache-Status: MISS` = fetched and cached.
`X-Distfiles-Source: upstream` + `X-Cache-Status: HIT` = served from cache.

## Maintenance

- Cache size cap is 200 GB (`inactive=90d`); usage:
  `sudo du -sh /var/cache/portuale-distfiles-mirror`.
- To promote a cached miss into the shared store (so local portage sees
  it), it's an ordinary file under the proxy cache but with the hashed
  cache filename — simplest is to re-download the few you want directly
  into `/.gentoo/cache/distfiles` rather than fish them out.
- To disable: `sudo systemctl stop nginx` (or remove
  `/etc/nginx/conf.d/distfiles-mirror.conf` and `systemctl reload nginx`).
