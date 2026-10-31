#!/usr/bin/env bats
# Checks tools/backup.sh's guards, and that what it runs on the TV only
# reads. None of these reach a TV.

setup() {
    BACKUP="$BATS_TEST_DIRNAME/../backup.sh"
    REPO=$(cd "$BATS_TEST_DIRNAME/../.." && pwd -P)
    export LUMEN_ENV_FILE=/dev/null
    unset TV_HOST TV_KEY
}

@test "an argument fails with usage" {
    run "$BACKUP" now
    [ "$status" -eq 2 ]
    [[ "$output" == *"usage: tools/backup.sh"* ]]
}

@test "a folder inside the repository is refused, and nothing is made" {
    LUMEN_BACKUP_DIR="$REPO/tools/out/backup-test/deeper" run "$BACKUP"
    [ "$status" -eq 1 ]
    [[ "$output" == *"refusing to save inside the repository"* ]]
    [ ! -e "$REPO/tools/out/backup-test" ]
}

@test "a relative folder inside the repository is refused" {
    cd "$REPO/tools"
    LUMEN_BACKUP_DIR=../docs/backup-test run "$BACKUP"
    [ "$status" -eq 1 ]
    [[ "$output" == *"refusing to save inside the repository"* ]]
    [ ! -e "$REPO/docs/backup-test" ]
}

@test "the repository itself is refused" {
    LUMEN_BACKUP_DIR="$REPO" run "$BACKUP"
    [ "$status" -eq 1 ]
    [[ "$output" == *"refusing to save inside the repository"* ]]
}

@test "a folder outside the repository gets as far as connecting" {
    LUMEN_BACKUP_DIR="$BATS_TEST_TMPDIR/backups" run "$BACKUP"
    [ "$status" -ne 0 ]
    [[ "$output" == *"TV_HOST is not set"* ]]
}

@test "the scripts run on the TV change nothing and write no file" {
    for f in "$BATS_TEST_DIRNAME"/../backup/*.sh; do
        run sh -c "grep -v -E '^[[:space:]]*#' '$f' | grep -n -E \
            '(^|[;&|(\`[:space:]])(rm|mv|cp|ln|mount|umount|touch|mkdir|rmdir|tee|dd|chmod|chown|kill|killall|pkill|reboot|sysctl|insmod|modprobe|rmmod|ifconfig|passwd|initctl (start|stop|restart|emit))([[:space:]]|$)|ip6?tables .*-[AIDRNXPEFZ]'"
        [ "$status" -eq 1 ]
        run sh -c "grep -v -E '^[[:space:]]*#' '$f' \
            | sed -e 's/[0-9]*>&[0-9]//g' -e 's|[0-9]*>/dev/null||g' | grep -n '>'"
        [ "$status" -eq 1 ]
    done
}

@test "the scripts run on the TV call luna-send only through their get/list guard" {
    for f in "$BATS_TEST_DIRNAME"/../backup/*.sh; do
        [ "$(grep -v -E '^[[:space:]]*#' "$f" | grep -c 'luna-send')" -eq 1 ]
        run sh -c "grep -v -E '^[[:space:]]*#' '$f' \
            | grep -o -E '^[[:space:]]*luna [A-Za-z0-9./]+' | grep -v -E '/(get|list)[A-Za-z]*\$'"
        [ "$status" -eq 1 ]
    done
}
