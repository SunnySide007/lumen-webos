#!/bin/sh
# Gives lumend its own luna role (decision 14), as Homebrew Channel's
# installer gives its service one: three files in the hub's developer
# folders, then a rescan.
#
#   tools/lumend.sh role install GROUP...
#
# (which puts the groups in LUMEND_GROUPS below and runs this with
# tools/tv.sh script).
#
# * roles.d/io.github.sunnyside007.lumend.json: the binary's path
#   (/var/lib/lumen/lumend/lumend, root's own folder), the two names it may
#   hold (the daemon's and `lumend try`'s), outbound to anything, inbound
#   from nothing: it offers no method.
# * client-permissions.d/io.github.sunnyside007.lumend.json: the groups,
#   chosen by the owner from `tools/lumend.sh groups`.
# * manifests.d/io.github.sunnyside007.lumend.json: the two, for the hub.
#
# The undo is role-remove.sh. Each file is written whole (a copy, then a
# rename); nothing else is touched. busybox sh.
set -eu

LUMEND_GROUPS='@LUMEND_GROUPS@'
ID=io.github.sunnyside007.lumend
LS2=/var/luna-service2-dev
BIN=/var/lib/lumen/lumend/lumend
ROLE=$LS2/roles.d/$ID.json
PERMS=$LS2/client-permissions.d/$ID.json
MANIFEST=$LS2/manifests.d/$ID.json

fail() {
    echo "role-install.sh: $*; nothing was changed" >&2
    exit 1
}

case $LUMEND_GROUPS in
    '' | *@*) fail "no groups: run it through tools/lumend.sh role install GROUP..." ;;
esac
list=
for g in $LUMEND_GROUPS; do
    case $g in
        *[!A-Za-z0-9._-]*) fail "a group name with odd characters" ;;
    esac
    list="$list${list:+,}\"$g\""
done
if [ ! -f $BIN ] || [ -L $BIN ]; then fail "no $BIN: tools/lumend.sh push first"; fi
for d in $LS2 $LS2/roles.d $LS2/client-permissions.d $LS2/manifests.d; do
    if [ ! -d "$d" ] || [ -L "$d" ]; then fail "$d is not a folder"; fi
done

# put FILE TEXT: FILE's new contents, whole.
put() {
    rm -f "$1.lumend-new"
    printf '%s\n' "$2" >"$1.lumend-new"
    chmod 644 "$1.lumend-new"
    mv -f "$1.lumend-new" "$1"
    echo "wrote $1"
}

put $ROLE "{\"exeName\":\"$BIN\",\"type\":\"regular\",\"allowedNames\":[\"$ID\",\"$ID.try\"],\"permissions\":[{\"service\":\"$ID\",\"outbound\":[\"*\"]},{\"service\":\"$ID.try\",\"outbound\":[\"*\"]}]}"
put $PERMS "{\"$ID*\":[$list]}"
put $MANIFEST "{\"id\":\"$ID\",\"version\":\"1.0.0\",\"roleFiles\":[\"$ROLE\"],\"clientPermissionFiles\":[\"$PERMS\"]}"

if command -v ls-control >/dev/null 2>&1; then
    ls-control scan-services && echo "the hub read them again (ls-control scan-services)"
else
    echo "no ls-control here: the hub reads them at the next boot"
fi
