#!/usr/bin/env bats
# Checks tools/studio.sh against a stub docker that records its arguments, so
# no image is built and no container runs.

setup() {
    ROOT=$(cd "$BATS_TEST_DIRNAME/../.." && pwd)
    STUDIO="$ROOT/tools/studio.sh"
    LOG="$BATS_TEST_TMPDIR/docker.log"
    TAG="lumen-studio:$(sha256sum "$ROOT/studio/Dockerfile" | cut -c1-12)"

    mkdir -p "$BATS_TEST_TMPDIR/bin"
    cat >"$BATS_TEST_TMPDIR/bin/docker" <<'EOF'
#!/bin/sh
# One line per call: MSYS_NO_PATHCONV, then each argument followed by "|".
{ printf 'MSYS_NO_PATHCONV=%s ' "${MSYS_NO_PATHCONV-}"; printf '%s|' "$@"; echo; } >>"$STUB_LOG"
if [ "$1" = build ]; then
    cat >"$STUB_LOG.stdin"
fi
if [ "$1 $2" = "image inspect" ]; then
    [ "$STUB_IMAGE" = present ]
fi
EOF
    chmod +x "$BATS_TEST_TMPDIR/bin/docker"
    export PATH="$BATS_TEST_TMPDIR/bin:$PATH"
    export STUB_LOG="$LOG" STUB_IMAGE=present
}

@test "the image is tagged with a hash of studio/Dockerfile" {
    run "$STUDIO" true
    [ "$status" -eq 0 ]
    grep -q "|$TAG|true|$" "$LOG"
}

@test "a missing image is built from studio/Dockerfile" {
    STUB_IMAGE=absent run "$STUDIO" true
    [ "$status" -eq 0 ]
    grep -q "^MSYS_NO_PATHCONV=[^ ]* build|-t|$TAG|-|$" "$LOG"
    cmp "$LOG.stdin" "$ROOT/studio/Dockerfile"
    [[ "$output" == *"Building $TAG"* ]]
}

@test "an existing image is not rebuilt" {
    run "$STUDIO" true
    [ "$status" -eq 0 ]
    ! grep -q " build|" "$LOG"
}

@test "the repository is mounted at /work and paths are not rewritten" {
    run "$STUDIO" true
    [ "$status" -eq 0 ]
    grep -q "^MSYS_NO_PATHCONV=1 run|--rm|-v|$ROOT:/work|-w|/work|$TAG|true|$" "$LOG"
}

@test "arguments reach the container unchanged" {
    run "$STUDIO" python3 -c 'print("a b")' "two  spaces"
    [ "$status" -eq 0 ]
    grep -qF "|$TAG|python3|-c|print(\"a b\")|two  spaces|" "$LOG"
}

@test "no arguments opens a shell" {
    run "$STUDIO"
    [ "$status" -eq 0 ]
    grep -q "|$TAG|bash|$" "$LOG"
}

@test "a failed build stops before anything runs" {
    cat >"$BATS_TEST_TMPDIR/bin/docker" <<'EOF'
#!/bin/sh
printf '%s|' "$@" >>"$STUB_LOG"; echo >>"$STUB_LOG"
[ "$1" != build ] && [ "$1 $2" != "image inspect" ]
EOF
    run "$STUDIO" true
    [ "$status" -ne 0 ]
    ! grep -q "^run|" "$LOG"
}
