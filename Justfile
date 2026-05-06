set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

compose := "docker compose"
dev_service := "dev"

default:
    @just --list

# Docker-authoritative workflows
build:
    {{ compose }} build fuse

check:
    {{ compose }} run --build --rm {{ dev_service }} cargo check --all-targets --all-features

test:
    {{ compose }} run --build --rm {{ dev_service }} cargo test --all-targets --all-features

lint:
    {{ compose }} run --build --rm {{ dev_service }} cargo clippy --all-targets --all-features -- -D warnings

fmt:
    {{ compose }} run --build --rm {{ dev_service }} cargo fmt --all

fmt-check:
    {{ compose }} run --build --rm {{ dev_service }} cargo fmt --all -- --check

ci: fmt-check lint test

up:
    mkdir -p sample cache virtual
    {{ compose }} up -d --build fuse

down:
    {{ compose }} down --remove-orphans

logs:
    {{ compose }} logs -f fuse

shell:
    {{ compose }} run --build --rm {{ dev_service }} bash

runtime-shell:
    {{ compose }} run --rm --entrypoint bash fuse

clean:
    {{ compose }} down --remove-orphans

clean-all:
    {{ compose }} down --volumes --remove-orphans

# Host development convenience only; Docker recipes above define the runtime.
host-check:
    cargo check --all-targets --all-features

host-test:
    cargo test --all-targets --all-features

host-lint:
    cargo clippy --all-targets --all-features -- -D warnings

host-fmt:
    cargo fmt --all

host-fmt-check:
    cargo fmt --all -- --check
