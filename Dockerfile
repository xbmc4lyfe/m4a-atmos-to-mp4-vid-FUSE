# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS builder

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libfuse3-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/workspace/target \
    cargo build --release --locked \
    && cp target/release/m4a_atmos_to_mp4_vid_fuse /usr/local/bin/m4a-atmos-fuse

FROM rust:1-bookworm AS dev

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        fuse3 \
        git \
        libfuse3-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add clippy rustfmt

WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY src ./src

CMD ["cargo", "test", "--all-targets", "--all-features"]

FROM debian:bookworm-slim AS runtime

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        fuse3 \
        rclone \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /mnt/source /mnt/virtual /var/cache/m4a-atmos-fuse

COPY --from=builder /usr/local/bin/m4a-atmos-fuse /usr/local/bin/m4a-atmos-fuse

VOLUME ["/mnt/source", "/mnt/virtual", "/var/cache/m4a-atmos-fuse"]

ENTRYPOINT ["/usr/local/bin/m4a-atmos-fuse"]
CMD ["--source", "/mnt/source", "--mount", "/mnt/virtual", "--cache", "/var/cache/m4a-atmos-fuse", "--foreground"]
