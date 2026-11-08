#!/bin/sh
# The TV for e2e/phone.spec.ts, in the build container: lumen-remote as root,
# dropping to its own user as on the TV, in front of `lumen-ctl serve --bus
# fake`. The phone tests start it through tools/dev.sh with the port
# published, and drive the PC's side with `docker exec ... lumen-ctl serve`
# on the same state folder. Nothing reaches a TV.
#
#   tools/dev.sh sh remote/ui/e2e/phone-tv.sh PORT
#
# It wants remote/ui/dist (npm run build); it builds the two binaries.
set -eu

port=${1:-18443}
cd /work/shell
cargo build --quiet -p lumen-remote -p lumen-ctl
bin=/work/shell/target/debug

# lumen-remote wants root's files, no wider than 0755 (0600 for the key),
# which a folder mounted from Windows doesn't give.
d=/tmp/lumen
rm -rf "$d"
mkdir -p "$d/state"
fixtures=/work/shell/crates/lumen-remote/tests/fixtures
cp "$fixtures/server.crt" "$d/tls.crt"
cp "$fixtures/server.key" "$d/tls.key"
cp -r /work/remote/ui/dist "$d/web"
cp "$bin/lumen-ctl" "$d/lumen-ctl"
chmod -R go-w "$d"
chmod 600 "$d/tls.key"
chmod 755 "$d/lumen-ctl"

# Docker's bridge reaches the container from 172.16.0.0/12.
exec "$bin/lumen-remote" \
    --listen "0.0.0.0:$port" --allow 172.16.0.0/12 --allow 127.0.0.0/8 \
    --name 127.0.0.1 --name localhost \
    --cert "$d/tls.crt" --key "$d/tls.key" --web "$d/web" \
    --ctl "$d/lumen-ctl" --jail "$d/empty" --log "$d/remote.log" \
    --ctl-arg --bus --ctl-arg fake --ctl-arg --alive-ms --ctl-arg 1000 \
    --ctl-arg --state --ctl-arg "$d/state" \
    --ctl-arg --log --ctl-arg "$d/ctl.log" \
    --ctl-arg --config --ctl-arg "$d/none"
