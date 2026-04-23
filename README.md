# Mirage

_Create the illusion of local files in the desert of your HTTP streams._

Mirage is an HTTP front-end to an Xtream Codes VOD and series API. It serves HTML directory listings that [rclone’s `http` backend](https://rclone.org/http/) understands. For **video files**, Mirage checks that the URL matches the catalog, then normally responds with **`307 Temporary Redirect`** to your provider’s real **`/movie/...`** (films) or **`/series/...`** (episodes) URL.

## Environment variables

| Variable                                | Required | Default                   | Description                                                                                                                                                     |
| --------------------------------------- | -------- | ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `XTREAM_BASE_URL`                       | **Yes**  | —                         | Xtream server base URL (scheme + host, optional port). Trailing slashes are stripped; do **not** include `/player_api.php`. Example: `https://iptv.example.com` |
| `XTREAM_USERNAME`                       | **Yes**  | —                         | Xtream username for `player_api.php` and stream URLs                                                                                                            |
| `XTREAM_PASSWORD`                       | **Yes**  | —                         | Xtream password                                                                                                                                                 |
| `LISTEN`                                | No       | `127.0.0.1:8080`          | Socket to bind. Use `0.0.0.0:8080` only if you intend to expose Mirage on the network                                                                           |
| `MIRAGE_TV_CATALOG_PATH`                | No       | `data/tv_catalog.rkyv`    | Filesystem path for the on-disk TV catalog snapshot                                                                                                             |
| `MIRAGE_TV_REFRESH_SECS`                | No       | `43200`                   | How often the background job rebuilds the TV catalog (12 hours, minimum 1)                                                                                      |
| `MIRAGE_MOVIE_CATALOG_PATH`             | No       | `data/movie_catalog.rkyv` | Filesystem path for the on-disk Movie catalog snapshot                                                                                                          |
| `MIRAGE_MOVIE_REFRESH_SECS`             | No       | `43200`                   | How often the background job rebuilds the Movie catalog (12 hours, minimum 1)                                                                                   |
| `MIRAGE_UPSTREAM_MIN_INTERVAL_MS`       | No       | `1000`                    | Minimum spacing between **Xtream API** JSON requests process-wide (minimum 1)                                                                                   |
| `MIRAGE_UPSTREAM_MAX_INFLIGHT`          | No       | `1`                       | Max concurrent Xtream API JSON requests (minimum 1)                                                                                                             |
| `MIRAGE_STREAM_PROBE_USE_UPSTREAM_HEAD` | No       | `off`                     | When `1`/`true`/`yes`/`on`, **`HEAD`** to Mirage is answered with **`307`** to the provider (client **`HEAD`**s upstream) instead of a ranged **`GET`** probe   |

### Test mode

Mirage still performs a normal `get_vod_categories` / `get_vod_streams` HTTP call, but **after parsing** it keeps only a small prefix so listings and mounts stay tiny. This avoids accidentally walking thousands of folders while you tune rclone or your media server.

| Variable                     | Required | Default | Description                                                                                            |
| ---------------------------- | -------- | ------- | ------------------------------------------------------------------------------------------------------ |
| `MIRAGE_TEST_MODE`           | No       | `off`   | When `1`, `true`, `yes`, or `on` (case-insensitive), caps below apply                                  |
| `MIRAGE_TEST_MAX_CATEGORIES` | No       | `1`     | Max categories from `get_vod_categories` and `get_series_categories` (minimum 1)                       |
| `MIRAGE_TEST_MAX_VOD`        | No       | `10`    | Max movies per category from `get_vod_streams` (minimum 1)                                             |
| `MIRAGE_TEST_MAX_SERIES`     | No       | `10`    | TV catalog: after merging `get_series` across categories, keep only the **first N** series (minimum 1) |
| `MIRAGE_TEST_MAX_EPISODES`   | No       | `10`    | Max episodes **per season** after `get_series_info` (minimum 1)                                        |

In test mode the home page is labeled **Mirage (test mode)**, the **Movies** and **TV Shows** links both use the limited-catalog labels, and startup logs a short warning with the active caps.

**Logging:** If `RUST_LOG` is unset, Mirage defaults to `mirage=info`.

## Run Mirage

```bash
cargo run --release
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

2. Quick test without mounting:

   ```bash
   rclone lsd mirage:
   rclone lsd mirage:movies
   rclone lsd mirage:tv
   ```

## Mount with rclone

[`rclone mount`](https://rclone.org/commands/rclone_mount/) builds a FUSE (or Windows equivalent) filesystem on top of the remote.

Example Linux mount (replace `mirage:` with your remote name, and `/mnt/mirage` with your mountpoint):

```bash
rclone mount mirage: /mnt/mirage \
  --read-only \
  --dir-cache-time 24h \
  --vfs-cache-mode full \
  --vfs-cache-max-size 20G \
  --vfs-cache-max-age 2h \
  --vfs-read-chunk-size 16M \
  --vfs-read-chunk-size-limit 64M \
  --buffer-size 16M \
  --no-checksum \
  --no-modtime
```

### Why these flags?

- `--read-only`: Prevents writes, deletes, or renames through the mount so media scanners and players can only read files.
<!-- - `--tpslimit 4 --tpslimit-burst 4`: Rate-limits scans and metadata bursts (for example, Plex library scans) so you are less likely to trip provider concurrency/request limits. -->
- `--vfs-read-chunk-size 16M --vfs-read-chunk-size-limit 64M`: Starts with moderate read-ahead, grows for sequential playback, and caps growth so a single stream does not request huge chunks.
- `--vfs-cache-mode full`: Enables disk-backed VFS reads, which is key for seeks and rewinds without re-fetching the same ranges upstream.
- `--vfs-cache-max-size 20G --vfs-cache-max-age 2h`: Keeps a bounded rolling cache; old or unused chunks are evicted automatically.
- `--buffer-size 16M`: Uses a small per-open-file RAM buffer before disk cache for smoother playback without high memory usage.
- `--no-checksum --no-modtime`: Skips expensive/unsupported metadata checks for HTTP remotes, reducing extra upstream requests.
<!-- - `--allow-other`: Lets other local users/processes (for example Plex/Jellyfin services) access the mount, not just the user who started `rclone mount`. (requires `user_allow_other` in your `fuse.conf`) -->

Unmount (Linux FUSE):

```bash
fusermount3 -u /mnt/mirage
```

## Run with systemd

The repository includes example unit files in `units/` for running Mirage as a rootless Podman Quadlet and mounting it with an rclone user service.

### Prerequisites

- Podman with Quadlet support (user services)
- systemd user session (`systemctl --user ...`)
- rclone configured with an `http` remote that points to `http://127.0.0.1:8080/`
- FUSE tools available (`fusermount`)

### 1. Install the unit files

```bash
mkdir -p ~/.config/containers/systemd ~/.config/systemd/user
cp units/mirage.container ~/.config/containers/systemd/mirage.container
cp units/mirage.volume ~/.config/containers/systemd/mirage.volume
cp units/rclone.service ~/.config/systemd/user/rclone.service
```

### 2. Create `.env.mirage`

`mirage.container` uses:

```ini
EnvironmentFile=./.env.mirage
```

That means `.env.mirage` must exist next to `mirage.container` at:

```text
~/.config/containers/systemd/.env.mirage
```

Minimum required variables in `.env.mirage`:

```dotenv
XTREAM_BASE_URL=https://iptv.example.com
XTREAM_USERNAME=your_username
XTREAM_PASSWORD=your_password
```

Notes:

- Use plain `KEY=VALUE` lines (no `export`)
- Keep the file readable only by your user account:

  ```bash
  chmod 600 ~/.config/containers/systemd/.env.mirage
  ```

### 3. Reload and start user units

```bash
systemctl --user daemon-reload
systemctl --user enable --now rclone.service
```

`rclone.service` has `Requires=mirage.service` and `After=mirage.service`, so starting rclone will pull Mirage up first.

### 4. Verify and troubleshoot

```bash
systemctl --user status mirage.service rclone.service
journalctl --user -u mirage.service -u rclone.service -f
```

Optional: keep user services running after logout:

```bash
loginctl enable-linger "$USER"
```

## Catalog Layout & Media Server Compatibility

Mirage presents a virtual file system that organizes your provider's media into a clean, predictable structure designed to be easily read by media servers like **Plex** and **Jellyfin**.

To do this, Mirage embeds unique identifiers straight into the folder and file names. Media servers use these tags (like `{tmdb-12345}`) to skip fallback text-matching and instantly fetch the correct metadata.

### Movie Layout

Movies are organized by category, then by a folder containing the movie file itself:

```text
movies/
  <Category Name>/
    <Title> (<Year>) {tmdb-<id>} [tmdbid-<id>] {vodid-<id>}/
       <Title> (<Year>) {tmdb-<id>} [tmdbid-<id>] {vodid-<id>}.<ext>
```

_Example:_ `movies/Action/The Matrix (1999) {tmdb-603} [tmdbid-603] {vodid-10042}/The Matrix (1999) {tmdb-603} [tmdbid-603] {vodid-10042}.mp4`

### TV Show Layout

TV Shows omit the category to avoid duplication and are structured into season folders:

```text
tv/
  <Show Title> (<Year>) {tmdb-<id>} [tmdbid-<id>] {seriesid-<id>}/
    Season <XX>/
      <Episode Title> {epid-<id>}.<ext>
```

_Example:_ `tv/The Office (2005) {tmdb-2316} [tmdbid-2316] {seriesid-824}/Season 01/The Office (2005) - S01E01 - Pilot {epid-98102}.mkv`

### Provider Metadata Requirements

To get the best experience with full metadata matching, your IPTV provider's Xtream API should accurately populate the standard application fields. Mirage relies heavily on the following fields:

**For Movies:**

- `name` / `title`: Used for the base name.
- `year`, `releaseDate`, or year in the title: Appended as `(YYYY)` to help with matching.
- `tmdb_id` / `tmdbId`: Injected as `{tmdb-id}` and `[tmdbid-id]` which guarantees a perfect match in Plex/Jellyfin.
- `container_extension`: Dictates the file extension (`.mp4`, `.mkv`, etc.).
- `stream_id`: Required to generate the playable link.

**For TV Shows:**

- `name`: Used for the show folder name.
- `releaseDate`: Used for the show year.
- `tmdb`: Used for show-level matching.
- `series_id`: Required to organize the show and fetch episode info.
- `season` & `episode_num`: Required to organize episodes into `Season XX` folders and for metadata ordering.
- `id` (Episode Stream ID): Required to generate the playable link.
