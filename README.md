# Mirage

_Create the illusion of local files in the desert of your HTTP streams._

Mirage is an HTTP front-end to an Xtream Codes VOD and series API. It serves HTML directory listings that [rclone’s `http` backend](https://rclone.org/http/) understands, and proxies video `GET`/`HEAD` (including `Range`) to your provider’s `/movie/...` (films) and `/series/...` (TV episodes) URLs so everything stays on one host from rclone’s point of view.

When a client (e.g. rclone) sends **`HEAD` to Mirage** for a video file, Mirage probes the **provider** with a **`GET` and `Range: bytes=0-0`** (upstream **`HEAD` is off by default**; set `MIRAGE_STREAM_PROBE_USE_UPSTREAM_HEAD` if your provider supports it and you want to try it first). The probe must return **`206 Partial Content`** with **`Content-Range`** (including total size), **`Content-Type`**, and **`Accept-Ranges: bytes`**. If that fails, Mirage responds with **502** and an error message—there is no silent fallback. Probe results are **cached per stream URL for 15 minutes**. Full **`GET`** playback is a normal proxied stream; concurrent upstream video requests are limited by **`MIRAGE_STREAM_MAX_INFLIGHT`**.

**Small `Range` reads (Jellyfin / ffprobe):** FUSE and tools often request **`bytes=0-0`** or a few KB at the start of a file. Upstreams may respond with **206** and literally **one byte**, which breaks **ffprobe** (MKV EBML / MP4 headers need more data). Mirage **widens** those requests to at least **256 KiB** from offset 0 when forwarding upstream (using cached `Content-Length` to cap at EOF when known). You may still see **`error writing a body to connection`** in logs when the client closes the socket after it has read enough- that is normal.

## Environment variables

| Variable                                | Required | Description                                                                                                                                                     |
| --------------------------------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `XTREAM_BASE_URL`                       | **Yes**  | Xtream server base URL (scheme + host, optional port). Trailing slashes are stripped; do **not** include `/player_api.php`. Example: `https://iptv.example.com` |
| `XTREAM_USERNAME`                       | **Yes**  | Xtream username for `player_api.php` and stream URLs                                                                                                            |
| `XTREAM_PASSWORD`                       | **Yes**  | Xtream password                                                                                                                                                 |
| `LISTEN`                                | No       | Socket to bind (default `127.0.0.1:8080`). Use `0.0.0.0:8080` only if you intend to expose Mirage on the network                                                |
| `MIRAGE_TV_CATALOG_PATH`                | No       | Filesystem path for the on-disk TV catalog snapshot (default `data/tv_catalog.rkyv`)                                                                            |
| `MIRAGE_TV_REFRESH_SECS`                | No       | How often the background job rebuilds the TV catalog (default **43200** = 12 hours, minimum 1)                                                                  |
| `MIRAGE_UPSTREAM_MIN_INTERVAL_MS`       | No       | Minimum spacing between **Xtream API** JSON requests process-wide (default **300**, minimum 1)                                                                  |
| `MIRAGE_UPSTREAM_MAX_INFLIGHT`          | No       | Max concurrent Xtream API JSON requests (default **1**, minimum 1)                                                                                              |
| `MIRAGE_STREAM_MAX_INFLIGHT`            | No       | Max concurrent upstream **video** HTTP requests (`GET`/`HEAD` probe); additional requests **wait** for a slot (default **16**, minimum 1)                       |
| `MIRAGE_STREAM_PROBE_USE_UPSTREAM_HEAD` | No       | When `1`/`true`/`yes`/`on`, try upstream **`HEAD`** before the ranged `GET` probe if the `HEAD` response is strictly valid (default **off**)                    |

### Test mode

Mirage still performs a normal `get_vod_categories` / `get_vod_streams` HTTP call (the provider may return a large JSON body), but **after parsing** it keeps only a small prefix so listings and mounts stay tiny. This avoids walking thousands of folders while you tune rclone or Plex.

| Variable                     | Required | Description                                                                                                            |
| ---------------------------- | -------- | ---------------------------------------------------------------------------------------------------------------------- |
| `MIRAGE_TEST_MODE`           | No       | When `1`, `true`, `yes`, or `on` (case-insensitive), caps below apply                                                  |
| `MIRAGE_TEST_MAX_CATEGORIES` | No       | Max categories from `get_vod_categories` and `get_series_categories` (default **1**, minimum 1)                        |
| `MIRAGE_TEST_MAX_VOD`        | No       | Max movies per category from `get_vod_streams` (default **10**, minimum 1)                                             |
| `MIRAGE_TEST_MAX_SERIES`     | No       | TV catalog: after merging `get_series` across categories, keep only the **first N** series (default **10**, minimum 1) |
| `MIRAGE_TEST_MAX_EPISODES`   | No       | Max episodes **per season** after `get_series_info` (default **10**, minimum 1)                                        |

In test mode the home page is labeled **Mirage (test mode)**, the **Movies** and **TV Shows** links both use the limited-catalog labels, and startup logs a short warning with the active caps.

**Logging:** If `RUST_LOG` is unset, Mirage defaults to `mirage=debug,tower_http=debug,axum=trace`. Override with `RUST_LOG` when you want quieter logs.

## Run Mirage

```bash
cargo run --release
# or after install:
# mirage
```

## Configure rclone (`http` remote)

1. Create a remote of type **http** ([upstream docs](https://rclone.org/http/)):

   ```bash
   rclone config
   ```

   Choose **http**, then set **url** to Mirage’s **root** including a **trailing slash** (avoids an extra `HEAD` to decide whether the root is a file or directory):

   ```text
   http://127.0.0.1:8080/
   ```

2. Quick test without a config file:

   ```bash
   rclone lsd :http,url='http://127.0.0.1:8080/':
   rclone lsd :http,url='http://127.0.0.1:8080/':movies
   rclone lsd :http,url='http://127.0.0.1:8080/':tv
   ```

   TV libraries follow common **Plex / Jellyfin** layout under `tv/`: `/tv/` lists all shows; each show is `Show Name (year) … {seriesid-…}/Season 01/…` with episode filenames containing `S##E##` and `{epid-…}` before the extension. Until the first catalog snapshot is ready, `/tv/` returns **503** so scanners do not see an empty list as “everything deleted.”

3. **rclone `--http-no-head` (optional):** This flag applies to **rclone → Mirage**, not Mirage → your IPTV provider. You do **not** need it merely because the provider lacks `HEAD`—Mirage probes with **ranged `GET`** by default. Consider [`--http-no-head`](https://rclone.org/http/#advanced-options) if **directory listings are slow** (each rclone `HEAD` to Mirage can trigger an upstream probe on cache miss), or if Mirage returns **502** on `HEAD` and rclone’s stat/listing breaks—in that case `no_head` avoids relying on `HEAD` to Mirage (file sizes may stay unknown until a read; it does not fix a broken upstream).

## Mount with rclone (VFS caching)

[`rclone mount`](https://rclone.org/commands/rclone_mount/) builds a FUSE (or Windows equivalent) filesystem on top of the remote. For **read-only HTTP + video**, you usually want **VFS read caching** so players can seek and rclone does not re-download the same ranges from Mirage for every small read.

You can figure out the optimal flags from the ([VFS file caching docs](https://rclone.org/commands/rclone_mount/#vfs-file-caching))

Example **Linux** mount (replace `mirage:` with your remote name, and `/mnt/mirage` with your mountpoint):

```bash
mkdir -p /mnt/mirage

rclone mount mirage: /mnt/mirage \
  --vfs-cache-mode full \
  --vfs-read-ahead 128M \
  --vfs-cache-max-size 50G \
  --vfs-cache-max-age 24h \
  --dir-cache-time 12h \
  --cache-dir "$HOME/.cache/rclone/mirage-vfs" \
  --log-level INFO \
  --daemon
```

- For **read-only browsing** and minimal disk use, `--vfs-cache-mode minimal` is lighter but **seek-heavy apps** (Plex/Jellyfin/transcodes) may still prefer `full`.
- With `--vfs-cache-mode off` (the default), rclone does not spool whole files to disk; long sequential reads still work, but **random access** inside large files is weaker.

Foreground mount (terminal stays open): use the command above. **Background** mount: add `--daemon` on supported platforms, or run under `systemd`, `screen`, or `tmux`.

Unmount (Linux FUSE):

```bash
fusermount -u /mnt/mirage
# or:
umount /mnt/mirage
```

If you are using docker / podman, you might want to try using [`rclone serve docker`](https://rclone.org/commands/rclone_serve_docker/) instead of `rclone mount`.
