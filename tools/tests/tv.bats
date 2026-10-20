#!/usr/bin/env bats
# Checks tools/tv.sh's argument and settings handling. None of these reach a TV:
# every case fails, or prints help, before SSH would run.

setup() {
    TV="$BATS_TEST_DIRNAME/../tv.sh"
    export LUMEN_ENV_FILE=/dev/null
    unset TV_HOST TV_KEY
}

@test "help lists the commands" {
    run "$TV" help
    [ "$status" -eq 0 ]
    [[ "$output" == *"install <file.ipk>"* ]]
    [[ "$output" != *"set -eu"* ]]
}

@test "an unknown command fails and shows help" {
    run "$TV" frobnicate
    [ "$status" -ne 0 ]
    [[ "$output" == *"unknown command: frobnicate"* ]]
}

@test "a missing TV_HOST is reported before anything connects" {
    run "$TV" check
    [ "$status" -ne 0 ]
    [[ "$output" == *"TV_HOST is not set"* ]]
}

@test "a missing key file is reported" {
    TV_HOST=192.0.2.1 TV_KEY=/nonexistent/key run "$TV" check
    [ "$status" -ne 0 ]
    [[ "$output" == *"no SSH key at /nonexistent/key"* ]]
}

@test "app ids with shell or JSON characters are refused" {
    key="$BATS_TEST_TMPDIR/key"
    touch "$key"
    TV_HOST=192.0.2.1 TV_KEY="$key" run "$TV" launch 'x"; reboot; "'
    [ "$status" -ne 0 ]
    [[ "$output" == *"not an app id"* ]]
}

@test "install refuses a file that does not exist" {
    key="$BATS_TEST_TMPDIR/key"
    touch "$key"
    TV_HOST=192.0.2.1 TV_KEY="$key" run "$TV" install /nonexistent.ipk
    [ "$status" -ne 0 ]
    [[ "$output" == *"no such file"* ]]
}
