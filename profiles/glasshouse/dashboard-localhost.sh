#!/bin/sh
# Runs on the TV (tools/tv.sh script profiles/glasshouse/dashboard-localhost.sh).
# Limits Glasshouse's dashboard, which has no password, to the TV itself:
# sets "host": "127.0.0.1" in /var/lib/tvweb/config.json, keeping the first
# original as config.json.lumen-bak, and restarts Glasshouse. Reach the
# dashboard afterwards with tools/tv.sh tunnel.
#
# Undo: tools/tv.sh run 'cp /var/lib/tvweb/config.json.lumen-bak
#       /var/lib/tvweb/config.json && /var/lib/tvweb/tvwebctl restart'
set -eu

CONFIG=/var/lib/tvweb/config.json
BACKUP=$CONFIG.lumen-bak
TVWEBCTL=/var/lib/tvweb/tvwebctl

[ -f "$CONFIG" ] || { echo "no $CONFIG: is Glasshouse installed and set up?" >&2; exit 1; }
[ -x "$TVWEBCTL" ] || { echo "no $TVWEBCTL" >&2; exit 1; }
NODE=$(command -v node || echo /usr/bin/node)
[ -x "$NODE" ] || { echo "no node on the TV" >&2; exit 1; }

[ -f "$BACKUP" ] || cp -p "$CONFIG" "$BACKUP"

# node 0.12 on webOS 4: ES5 only. Rewriting in place keeps the file's mode.
"$NODE" -e '
var fs = require("fs"), f = process.argv[1];
var c = JSON.parse(fs.readFileSync(f, "utf8"));
c.host = "127.0.0.1";
fs.writeFileSync(f, JSON.stringify(c, null, 2) + "\n");
' "$CONFIG"

grep '"host"' "$CONFIG"
"$TVWEBCTL" restart
sleep 3
echo "listening sockets on 8080:"
netstat -ltn 2>/dev/null | grep ':8080 ' || echo "(none yet; check again with tools/tv.sh run 'netstat -ltn')"
