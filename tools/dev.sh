#!/bin/sh
# Runs a command in the lumen-dev build container (docker/Dockerfile).
#
#   tools/dev.sh make ipk
#   tools/dev.sh make check
#   tools/dev.sh            # a shell in the container
#
# The image is tagged with a hash of the Dockerfile, so editing the Dockerfile
# builds a fresh image on the next run. Cargo's registry and the TV build's
# target directory live in named volumes, which keeps rebuilds fast.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
# Docker on Windows wants C:/... rather than Git Bash's /c/...
HOST_ROOT=$(cd "$ROOT" && { pwd -W 2>/dev/null || pwd; })

if command -v sha256sum >/dev/null 2>&1; then
    hash=$(sha256sum "$ROOT/docker/Dockerfile" | cut -c1-12)
else
    hash=$(shasum -a 256 "$ROOT/docker/Dockerfile" | cut -c1-12)
fi
image=lumen-dev:$hash

if ! docker image inspect "$image" >/dev/null 2>&1; then
    echo "Building $image (first run downloads the webOS toolchain, ~365 MB)..." >&2
    docker build -t "$image" -f "$ROOT/docker/Dockerfile" "$ROOT/docker"
fi

# One target volume per checkout: worktrees all mount at /work, so a shared
# volume would let cargo reuse another tree's build output.
if command -v sha256sum >/dev/null 2>&1; then
    checkout=$(printf '%s' "$HOST_ROOT" | sha256sum | cut -c1-8)
else
    checkout=$(printf '%s' "$HOST_ROOT" | shasum -a 256 | cut -c1-8)
fi

tty=""
if [ -t 0 ] && [ -t 1 ]; then tty="-it"; fi
[ $# -gt 0 ] || set -- bash

# Stop Git Bash rewriting /work and other container paths.
export MSYS_NO_PATHCONV=1
# shellcheck disable=SC2086
exec docker run --rm $tty \
    -v "$HOST_ROOT:/work" \
    -v lumen-cargo-registry:/usr/local/cargo/registry \
    -v "lumen-target-$checkout:/work/shell/target" \
    -w /work \
    "$image" "$@"
