#!/usr/bin/env bats
# Where the three boot hooks log (profiles/privacy/lumen-privacy,
# profiles/privacy/lumen-guard, takeover/lumen-home): each to its own file in
# /var/lib/lumen/log, through the same two helpers, and never into /tmp, which
# every app's jail shares. Each hook's own tests run it, rotation and unsafe
# folders included; these read the code. Nothing reaches a TV.

ROOT="$BATS_TEST_DIRNAME/../.."
HOOKS="profiles/privacy/lumen-privacy profiles/privacy/lumen-guard takeover/lumen-home"
READERS="tools/guard.sh tools/privacy-hook.sh tools/takeover.sh profiles/privacy/guard-apply.sh takeover/home-apply.sh takeover/status.sh"

# Fails if the command succeeds: a bare `!` never fails a bats test.
not() {
    if "$@"; then
        echo "should have failed: $*"
        return 1
    fi
}

# Code without comments.
code() {
    sed -e 's/^[[:space:]]*#.*//' -e 's/[[:space:]]#.*//' "$1"
}

@test "no boot hook names /tmp, and each logs to its own file in /var/lib/lumen/log" {
    for h in $HOOKS; do
        name=${h##*/}
        not grep -n /tmp <(code "$ROOT/$h")
        grep -qx 'LOGDIR=/var/lib/lumen/log' "$ROOT/$h"
        grep -qx "LOG=\$LOGDIR/$name.log" "$ROOT/$h"
        grep -qx 'ROOT_UID=0' "$ROOT/$h"
        # The background work's output goes where log_to says, and only there.
        grep -qx 'OUT=$(log_to)' "$ROOT/$h"
        grep -qx ') </dev/null >>"$OUT" 2>&1 &' "$ROOT/$h"
        [ "$(grep -c '>>' "$ROOT/$h")" -eq 1 ]
    done
}

@test "the hooks share folder_ok and log_to word for word, and folder_ok is the stage's check" {
    for fn in folder_ok log_to; do
        first=
        for h in $HOOKS; do
            body=$(sed -n "/^$fn() {/,/^}/p" "$ROOT/$h")
            [ -n "$body" ] || {
                echo "no $fn in $h"
                return 1
            }
            [ -z "$first" ] || [ "$body" = "$first" ] || {
                echo "$fn differs in $h"
                return 1
            }
            first=$body
        done
    done
    stage=$(sed -n '/^stage_ok() {/,/^}/p' "$ROOT/takeover/hook-install.sh" |
        sed -e 's/^stage_ok() {$/folder_ok() {/' -e 's/echo "takeover: /echo "/')
    [ "$stage" = "$(sed -n '/^folder_ok() {/,/^}/p' "$ROOT/profiles/privacy/lumen-privacy")" ]
}

@test "log_to checks both folders before it touches a log, and writes only the log folder and the hook's two logs" {
    body=$(sed -n '/^log_to() {/,/^}/p' "$ROOT/profiles/privacy/lumen-privacy")
    run grep -Eo '\b(rm|mv|cp|touch|mkdir|rmdir|ln|chmod|tee|dd|truncate)\b[^;|&)]*[^;|&)[:space:]]' <<<"$body"
    [ "${lines[*]}" = 'mkdir -p "${LOGDIR%/*}" mkdir -p "$LOGDIR" mv -f "$LOG" "$LOG.1"' ]
    run grep -Eo '>>?[[:space:]]*[^[:space:]&;)|]+' <<<"$body"
    [ "${lines[*]}" = '>"$LOG"' ]
    # In this order: /var/lib/lumen, the log folder made root's alone, that
    # folder, then the last log moved and the new one made. Any step that
    # fails sends the run's output to /dev/null.
    flat=$(tr -s ' \n' '  ' <<<"$body")
    [[ "$flat" == *'if folder_ok "${LOGDIR%/*}" >&2 && (umask 077 && mkdir -p "$LOGDIR") && folder_ok "$LOGDIR" >&2 && { [ ! -e "$LOG" ] || mv -f "$LOG" "$LOG.1"; } && : >"$LOG"; then echo "$LOG" else'*'echo /dev/null fi }'* ]]
}

@test "every reader of the hooks' logs reads them from /var/lib/lumen/log, and uninstall removes them" {
    for f in $READERS; do
        not grep -n '/tmp/lumen-\(guard\|privacy\|home\)' "$ROOT/$f"
        grep -q '/var/lib/lumen/log/lumen-\(guard\|privacy\|home\)\.log' "$ROOT/$f"
    done
    grep -qxF 'step run - "rm -rf /var/lib/lumen"' "$ROOT/tools/uninstall-all.sh"
}
