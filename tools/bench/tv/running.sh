# shellcheck shell=sh
# The ids of the running apps, one a line. Read-only.
luna luna://com.webos.applicationManager/running '{}' |
    sed -n 's/.*"id": *"\([^"]*\)".*/\1/p'
