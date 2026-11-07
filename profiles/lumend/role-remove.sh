#!/bin/sh
# Takes lumend's luna role away (role-install.sh's undo):
#
#   tools/lumend.sh role remove
#
# Deletes its three files from the hub's developer folders and has the hub
# read them again. Safe to run twice. busybox sh.
set -eu

ID=io.github.sunnyside007.lumend
LS2=/var/luna-service2-dev

for f in $LS2/roles.d/$ID.json $LS2/client-permissions.d/$ID.json $LS2/manifests.d/$ID.json; do
    if [ -e "$f" ] || [ -L "$f" ]; then
        rm -f "$f"
        echo "removed $f"
    else
        echo "no $f"
    fi
done
if command -v ls-control >/dev/null 2>&1; then
    ls-control scan-services && echo "the hub read them again (ls-control scan-services)"
fi
