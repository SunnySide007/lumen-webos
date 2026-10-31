#!/bin/sh
# Runs a command in the lumen-studio container (studio/Dockerfile): ffmpeg with
# 10-bit x265, numpy and OpenEXR, for the HDR content pipeline in studio/.
#
#   tools/studio.sh python3 studio/render.py --help
#   tools/studio.sh            # a shell in the container
#
# The image is tagged with a hash of the Dockerfile, so editing the Dockerfile
# builds a fresh image on the next run. Rendered frames and videos go under
# dist/, which git ignores.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
# Docker on Windows wants C:/... rather than Git Bash's /c/...
HOST_ROOT=$(cd "$ROOT" && { pwd -W 2>/dev/null || pwd; })

if command -v sha256sum >/dev/null 2>&1; then
    hash=$(sha256sum "$ROOT/studio/Dockerfile" | cut -c1-12)
else
    hash=$(shasum -a 256 "$ROOT/studio/Dockerfile" | cut -c1-12)
fi
image=lumen-studio:$hash

if ! docker image inspect "$image" >/dev/null 2>&1; then
    echo "Building $image (first run downloads ffmpeg, numpy and OpenEXR)..." >&2
    # The build copies nothing, so it needs no context: the Dockerfile comes on stdin.
    docker build -t "$image" - <"$ROOT/studio/Dockerfile"
fi

tty=""
if [ -t 0 ] && [ -t 1 ]; then tty="-it"; fi
[ $# -gt 0 ] || set -- bash

# Stop Git Bash rewriting /work and other container paths.
export MSYS_NO_PATHCONV=1
# shellcheck disable=SC2086
exec docker run --rm $tty \
    -v "$HOST_ROOT:/work" \
    -w /work \
    "$image" "$@"
