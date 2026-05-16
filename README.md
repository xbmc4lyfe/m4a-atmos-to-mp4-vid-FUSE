# M4A Atmos to MKV FUSE WebDAV

![CodeRabbit Pull Request Reviews](https://img.shields.io/coderabbit/prs/github/Appz4Fun/m4a-atmos-to-mp4-vid-FUSE?utm_source=oss&utm_medium=github&utm_campaign=Appz4Fun%2Fm4a-atmos-to-mp4-vid-FUSE&labelColor=171717&color=FF570A&link=https%3A%2F%2Fcoderabbit.ai&label=CodeRabbit+Reviews)

This project packages a Rust FUSE filesystem in Docker. It scans a read-only source tree for eligible `.m4a` Dolby Atmos EAC3 JOC audio files, exposes each accepted file as a virtual `.mkv`, and serves the mounted virtual output over WebDAV.

The generated MKV is intended to make audio-only Atmos files appear as video containers. On first access, the filesystem materializes a cached MKV with a still video stream and copied EAC3 audio by using `ffprobe` and `ffmpeg`.

## Docker Quick Start

1. Put source media under `./sample`, or edit the Compose source volume to point at the real media directory.
1. Start the FUSE filesystem and WebDAV server:

```sh
just up
```

1. Browse the WebDAV endpoint:

```text
http://localhost:9090/
```

The default Compose setup builds the runtime image, mounts `./sample` read-only, stores generated MKV files under `./cache`, and exposes the FUSE output from `./virtual` through WebDAV on `0.0.0.0:9090`.

## Volume Layout

- `./sample` mounts at `/mnt/source:ro` in the FUSE container. Put the source `.m4a` folder tree here, or edit the left side of that Compose volume to the real media directory.
- `./cache` mounts at `/var/cache/m4a-atmos-fuse` for generated MKV files and probe/transcode state.
- `./virtual` is the bind-mounted FUSE output at `/mnt/virtual`.
- The WebDAV service serves `/mnt/virtual` on `0.0.0.0:9090` and keeps directory listings cached in memory for 24 hours so very large libraries are not re-listed on every request.

The virtual tree preserves relative folder layout and exposes accepted source files as `.mkv`. A source file such as `Album/Track.m4a` is exposed as `Album/Track.mkv` when it passes eligibility checks.

## Runtime Requirements

The FUSE service runs with `/dev/fuse`, `SYS_ADMIN`, and unconfined AppArmor because mounting a FUSE filesystem from inside a container needs host kernel FUSE access.

The runtime image includes:

- the compiled Rust filesystem binary;
- Debian Trixie `ffmpeg` and `ffprobe`, which report the EAC3 Atmos profile used for eligibility checks;
- `fuse3`;
- `rclone` for WebDAV serving;
- CA certificates.

## Just Recipes

Docker recipes are the primary workflow:

```sh
just build
just check
just test
just lint
just fmt-check
just ci
just up
just logs
just down
```

Host recipes are development conveniences only and assume Rust plus native FUSE and ffmpeg tooling are already installed:

```sh
just host-check
just host-test
just host-lint
just host-fmt-check
```

## Limitations

- First access to a virtual `.mkv` can block while `ffprobe` and `ffmpeg` validate and materialize the cached file. Later playback can read from the generated cache.
- The source tree is indexed at container startup. Restart the service after adding new files.
- Only source `.m4a` files that probe as EAC3 and contain Atmos/JOC indicators are exposed.
- Generated video is a still image track with copied audio, not a real music video.
- Docker FUSE support depends on the host kernel, Docker runtime, and security settings.
