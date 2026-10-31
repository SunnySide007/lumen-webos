#!/usr/bin/env bats
# Checks tools/recon.sh's options, and that the privacy survey only reads.
# None of these reach a TV: the survey is read as text, never run.

setup() {
    RECON="$BATS_TEST_DIRNAME/../recon.sh"
    SURVEY="$BATS_TEST_DIRNAME/../recon/privacy.sh"
}

# The survey without its comment lines.
code() {
    grep -v -E '^[[:space:]]*#' "$SURVEY"
}

@test "an unknown option fails with usage before anything connects" {
    run "$RECON" --frobnicate
    [ "$status" -eq 2 ]
    [[ "$output" == *"usage: tools/recon.sh [--privacy]"* ]]
}

@test "the privacy survey runs no command that changes the TV" {
    run sh -c "grep -v -E '^[[:space:]]*#' '$SURVEY' | grep -n -E \
        '(^|[;&|(\`[:space:]])(rm|mv|cp|ln|mount|umount|touch|mkdir|rmdir|tee|dd|chmod|chown|kill|killall|pkill|reboot|sysctl|insmod|modprobe|rmmod|ifconfig|passwd|initctl (start|stop|restart|emit))([[:space:]]|$)'"
    [ "$status" -eq 1 ]
}

@test "the privacy survey changes no route, address or firewall rule" {
    run sh -c "grep -v -E '^[[:space:]]*#' '$SURVEY' | grep -n -E \
        'ip6?tables .*-[AIDRNXPEFZ]|ip( -[46])? (route|rule|addr|link|neigh) (add|del|change|replace|flush|set)'"
    [ "$status" -eq 1 ]
}

@test "the privacy survey writes no file" {
    # Every redirection goes to /dev/null or to another descriptor.
    run sh -c "grep -v -E '^[[:space:]]*#' '$SURVEY' \
        | sed -e 's/[0-9]*>&[0-9]//g' -e 's|[0-9]*>/dev/null||g' | grep -n '>'"
    [ "$status" -eq 1 ]
}

@test "the privacy survey calls luna-send only through its get/list guard" {
    [ "$(code | grep -c 'luna-send')" -eq 1 ]
    run sh -c "grep -v -E '^[[:space:]]*#' '$SURVEY' \
        | grep -o -E '^[[:space:]]*luna [A-Za-z0-9./]+' | grep -v -E '/(get|list)[A-Za-z]*\$'"
    [ "$status" -eq 1 ]
}
