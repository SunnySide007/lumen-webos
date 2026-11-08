#!/bin/sh
# Prints what the Control Center's Privacy panel shows (docs/specs/privacy.md,
# docs/specs/look-and-feel.md "The Control Center"): Lumen Guard's status
# file, the INPUT rules, and LG's reporting jobs. It runs as root, started
# through Homebrew Channel's exec with this fixed command and no arguments:
#
#   /bin/sh /media/developer/apps/usr/palm/applications/io.github.sunnyside007.lumen.shell/lumen-privacy-read.sh
#
# It only reads: nothing is written, started or stopped, and every part it
# cannot read is left empty (the panel then says so). The shell parses the
# output in lumen-core's privacy.rs:
#
#   <Guard's status file, as it is>
#   --- rules
#   <iptables -L INPUT -n>
#   --- jobs
#   <initctl status for each job, one line each>
set -u

# Tests only: a fake file system root in front of every path.
PREFIX=${LUMEN_PRIVACY_READ_PREFIX:-}

# Lumen Guard's status, rewritten every 10 s (shell/crates/lumen-guard).
STATUS=$PREFIX/var/lib/lumen/guard/status
IPTABLES=$PREFIX/usr/sbin/iptables
INITCTL=$PREFIX/sbin/initctl
# The jobs S1's hook stops (lumen-ctl's privacy.rs, JOBS).
JOBS="crashreportd rdxd uploadd remotelogger"

if [ -f "$STATUS" ]; then
    cat "$STATUS" 2>/dev/null
fi
echo "--- rules"
if [ -x "$IPTABLES" ]; then
    "$IPTABLES" -L INPUT -n 2>/dev/null
fi
echo "--- jobs"
if [ -x "$INITCTL" ]; then
    for job in $JOBS; do
        "$INITCTL" status "$job" 2>/dev/null
    done
fi
exit 0
