# Changelog

## 1.0.10 - 2026-05-06

- Package the Docker runtime on Debian Trixie so the bundled `ffprobe` reports Dolby Digital Plus Atmos profiles for real E-AC-3 JOC `.m4a` files.
- Add a committed 15-second `sample/sample.m4a` smoke-test file and wire Compose to use local `./sample`, `./cache`, and `./virtual` paths.
- Run FUSE and WebDAV from one runtime container on `0.0.0.0:9090`, with rclone directory listings cached in memory for large libraries.
- Fix probing and transcoding edge cases: remove the unsupported `ffprobe -nostdin` argument and scale odd cover-art dimensions to even H.264 sizes before muxing.
