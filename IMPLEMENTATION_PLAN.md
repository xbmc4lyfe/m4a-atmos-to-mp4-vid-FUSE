# M4A Atmos FUSE WebDAV Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Build a Docker-packaged Rust FUSE filesystem that exposes eligible source `.m4a` Dolby Atmos EAC3 JOC audio files as generated `.mkv` video containers and serves the FUSE output over WebDAV.

**Architecture:** The Rust binary mounts a read-only FUSE filesystem. It recursively indexes the source media tree, exposes only qualifying `.m4a` files as `.mkv` paths, and lazily runs `ffmpeg` into a cache file when a virtual `.mkv` is opened or read. Docker Compose runs a FUSE service with `/dev/fuse` access and a separate WebDAV service that serves the bind-mounted FUSE output on `0.0.0.0:9090`.

**Tech Stack:** Rust, `fuser`, `clap`, `walkdir`, `serde_json`, `sha2`, `anyhow`, `ffmpeg`, `ffprobe`, Docker, Docker Compose, `rclone serve webdav`, Just.

---

## File Responsibilities

- `src/main.rs`: command-line entry point and module wiring.
- `src/config.rs`: CLI and runtime path validation.
- `src/media.rs`: recursive source scan, virtual path mapping, ffprobe eligibility checks, inode table.
- `src/transcode.rs`: ffmpeg thumbnail extraction, black fallback image, `.mkv` materialization, cache freshness.
- `src/fs.rs`: read-only FUSE filesystem operations backed by indexed media and transcode cache files.
- `src/lib.rs`: testable module exports.
- `README.md`: project goal, Docker-first usage, host-development notes.
- `Dockerfile`: runtime image with Rust-built binary plus ffmpeg/fuse/rclone tools where needed.
- `docker-compose.yml`: FUSE service plus WebDAV service with source, cache, and mount bind volumes.
- `Justfile`: Docker-authoritative recipes for build, check, test, lint, format, run, logs, and cleanup.
- `.gitignore`: standard Rust ignores plus local dotfiles/env and `docs/superpowers/`, while keeping `.gitignore`.
- `.dockerignore`: keep build context small and avoid local secrets.

## Task 1: Repository Runtime Scaffolding

**Files:**
- Create: `README.md`
- Modify: `.gitignore`
- Modify: `.dockerignore`
- Modify: `Dockerfile`
- Modify: `docker-compose.yml`
- Modify: `Justfile`

- [ ] Replace `.gitignore` with standard Rust ignores and local-file protection:
  - ignore `/target/`, `Cargo.lock` only if this becomes a library is not desired, but for this binary keep `Cargo.lock` tracked;
  - ignore `.env`, `.env.*`, all root dotfiles by default via `.*`, then explicitly unignore `.gitignore`, `.dockerignore`, and `.github/`;
  - ignore `docs/superpowers/`, local media/cache/mount folders, logs, and OS/editor noise.
- [ ] Update `.dockerignore` to exclude `.git`, `target`, local env/dotfiles, media/cache/mount folders, logs, and `docs/superpowers/`, while allowing tracked source/config files.
- [ ] Make Docker the authoritative runtime:
  - a builder stage compiles the Rust binary;
  - a runtime stage includes `ffmpeg`, `ffprobe`, `fuse3`, and certificates;
  - default command mounts `/mnt/source` to `/mnt/virtual` with `/var/cache/m4a-atmos-fuse`.
- [ ] Update Compose:
  - `fuse` service gets `/dev/fuse`, `SYS_ADMIN`, and unconfined AppArmor;
  - source media bind mount defaults to `./media/source:/mnt/source:ro`;
  - cache bind mount defaults to `./media/cache:/var/cache/m4a-atmos-fuse`;
  - shared FUSE output bind mount defaults to `./media/mount:/mnt/virtual`;
  - the `webdav` service uses `rclone serve webdav /mnt/virtual` on `0.0.0.0:9090`;
  - WebDAV uses `--dir-cache-time 24h` and `--poll-interval 0` so massive directory listings are cached in memory;
  - Compose does not embed shell scripts; service commands are declarative argument lists.
- [ ] Replace Make-style workflows with Just-only recipes:
  - `just build`, `just check`, `just test`, `just lint`, `just fmt`, `just ci`;
  - `just up`, `just down`, `just logs`, `just shell`;
  - host recipes are explicitly marked as dev convenience.
- [ ] Write `README.md` with:
  - purpose and supported media assumptions;
  - Docker-first quick start;
  - source/cache/mount/WebDAV volume explanation;
  - expected output behavior;
  - known limitations around first-read transcode latency and source-file eligibility.

## Task 2: CLI, Media Index, And Eligibility Detection

**Files:**
- Create: `src/lib.rs`
- Create: `src/config.rs`
- Create: `src/media.rs`
- Modify: `src/main.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] Add dependencies: `anyhow`, `clap`, `fuser`, `libc`, `log`, `env_logger`, `serde`, `serde_json`, `sha2`, `time`, `walkdir`, and `tempfile` for tests.
- [ ] Implement CLI:
  - `--source /mnt/source`;
  - `--mount /mnt/virtual`;
  - `--cache /var/cache/m4a-atmos-fuse`;
  - `--foreground`.
- [ ] Implement recursive scanner:
  - walk source tree;
  - ignore non-files;
  - consider only `.m4a`, case-insensitive;
  - preserve relative folder layout;
  - expose `Album/Track.m4a` as `Album/Track.mkv`.
- [ ] Implement ffprobe eligibility:
  - run `ffprobe -v quiet -print_format json -show_streams -show_format <file>`;
  - accept only audio stream `codec_name=eac3`;
  - require probe JSON to contain either `joc` or `atmos` case-insensitively;
  - skip files that fail probing.
- [ ] Add unit tests for extension matching, recursive virtual path mapping, and ffprobe JSON acceptance/rejection.

## Task 3: Transcode Cache

**Files:**
- Create: `src/transcode.rs`
- Modify: `src/lib.rs`
- Modify: `src/media.rs`

- [ ] Generate a stable cache key from source absolute path, source size, and source mtime using SHA-256.
- [ ] If a fresh cached `.mkv` exists, reuse it.
- [ ] Materialize missing/stale cache files with `ffmpeg`:
  - first try to extract embedded cover art to a temporary JPEG;
  - if cover extraction fails, generate a black 1920x1080 still image;
  - create a single-image MKV with video track from the image and copied EAC3 audio;
  - write to a temporary output path and atomically rename it into place.
- [ ] Use ffmpeg command shape:
  - cover path: `ffmpeg -y -i input.m4a -an -vcodec copy cover.jpg`;
  - black fallback: `ffmpeg -y -f lavfi -i color=c=black:s=1920x1080:r=1 -frames:v 1 black.jpg`;
  - mux: `ffmpeg -y -loop 1 -i image.jpg -i input.m4a -map 0:v:0 -map 1:a:0 -c:v libx264 -tune stillimage -pix_fmt yuv420p -c:a copy -shortest output.tmp.mkv`.
- [ ] Add unit tests for cache key changes and command planning without requiring real media.

## Task 4: Read-Only FUSE Filesystem

**Files:**
- Create: `src/fs.rs`
- Modify: `src/main.rs`
- Modify: `src/lib.rs`
- Modify: `src/media.rs`
- Modify: `src/transcode.rs`

- [ ] Build an inode table with root inode `1`, directory entries, and file entries.
- [ ] Implement read-only FUSE operations:
  - `lookup`;
  - `getattr`;
  - `readdir`;
  - `open`;
  - `read`.
- [ ] On file `open` or first `read`, ensure the cached `.mkv` exists by calling the transcode cache.
- [ ] Serve file bytes from the cache using offset and size requested by FUSE.
- [ ] Return stable file sizes after cache materialization; for not-yet-materialized files, materialize before attributes when necessary.
- [ ] Reject write operations by omission/read-only mount options.
- [ ] Add tests for inode tree construction and cached read slicing.

## Task 5: Verification, Git, And PR

**Files:**
- Modify as needed based on verification failures.

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] Run `cargo test --all-targets --all-features`.
- [ ] Run `docker compose config`.
- [ ] Run `docker compose build fuse` if Docker is available.
- [ ] Configure repo-local git identity:
  - `user.name=xbmc4lyfe`;
  - `user.email=273732874+xbmc4lyfe@users.noreply.github.com`;
  - `commit.gpgsign=true`;
  - `gpg.format=ssh`;
  - signing key from the verified local xbmc4lyfe SSH signing setup.
- [ ] Set `origin` to `git@github.com:xbmc4lyfe/m4a-atmos-to-mp4-vid-FUSE.git`.
- [ ] Commit only intended project files with signed commits.
- [ ] Push a feature branch.
- [ ] Create a GitHub PR and report URL plus verification status.
