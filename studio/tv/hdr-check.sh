#!/bin/sh
# Plays a studio loop in LG's own video player and reports whether the TV
# switched into HDR to show it. Runs on the TV, as root:
#
#   tools/tv.sh script studio/tv/hdr-check.sh
#
# after `tools/tv.sh push <loop> /tmp/lumen-hdr/loop.mp4` (or loop.mkv); the
# steps are in studio/tv/README.md. It changes no setting: it closes and starts the
# player (com.webos.app.photovideo, the webOS 3-5 player) and reads the
# picture settings, whose "dimension" says which dynamic range the panel is
# in. /tmp is RAM, so a reboot leaves nothing behind. POSIX sh for busybox.
#
# Exit status: 0 HDR, 1 still SDR, 2 the player did not start or is not in
# front, 3 inconclusive (HDR before the player started).
#
# Every luna-send reads /dev/null: run as `sh -s` over SSH, a command that
# reads stdin would swallow the rest of this script.

dir=${LUMEN_HDR_DIR:-/tmp/lumen-hdr}
player=com.webos.app.photovideo
limit=30 # seconds to wait for playback to start and the picture mode to follow

video=
for candidate in "$dir/loop.mp4" "$dir/loop.mkv"; do
    if [ -f "$candidate" ]; then
        video=$candidate
        break
    fi
done
if [ -z "$video" ]; then
    echo "no loop in $dir: copy one to $dir/loop.mp4 first (studio/tv/README.md)"
    exit 1
fi

# The first "name": "value" pair in the picture settings, which is the one in
# the dimension block (settings repeats pictureMode further down).
field() {
    luna-send -n 1 -f luna://com.webos.settingsservice/getSystemSettings '{"category":"picture"}' </dev/null 2>&1 |
        grep -o "\"$1\": *\"[^\"]*\"" | head -n 1 | sed 's/^[^:]*: *"//; s/"$//'
}

state() {
    echo "$1: pictureMode=$(field pictureMode) dynamicRange=$(field dynamicRange)"
}

echo "loop: $video ($(wc -c <"$video") bytes)"

# A player left open from an earlier run would only come to the front, still
# playing the earlier loop: close it first.
luna-send -n 1 -f luna://com.webos.applicationManager/closeByAppId '{"id":"'$player'"}' </dev/null >/dev/null 2>&1
sleep 2
state before
range=$(field dynamicRange)
case $range in
    sdr | '') ;;
    *)
        echo "RESULT: inconclusive: the TV is in $range before the player starts"
        exit 3
        ;;
esac

case $video in
    *.mkv) mime=video/x-matroska ;;
    *) mime=video/mp4 ;;
esac
# The launch payload the player takes from LG's own media apps, with a file URL.
launch='{"id":"'$player'","params":{"payload":[{"fullPath":"file://'$video'","artist":"","subtitle":"",'
launch=$launch'"dlnaInfo":{"flagVal":4096,"cleartextSize":"-1","contentLength":"-1","opVal":1,'
launch=$launch'"protocolInfo":"http-get:*:'$mime':DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",'
launch=$launch'"duration":0},"mediaType":"VIDEO","thumbnail":"","deviceType":"DMR","album":"",'
launch=$launch'"fileName":"lumen-loop","lastPlayPosition":-1}]}}'
echo "launching $player"
answer=$(luna-send -n 1 -f luna://com.webos.applicationManager/launch "$launch" </dev/null 2>&1)
echo "$answer"
case $answer in
    *'"returnValue": true'* | *'"returnValue":true'*) ;;
    *)
        echo "RESULT: the player did not start"
        exit 2
        ;;
esac

# Wait for playback and the switch, which can take a while for 4K HEVC.
waited=0
while [ "$waited" -lt "$limit" ]; do
    sleep 2
    waited=$((waited + 2))
    range=$(field dynamicRange)
    case $range in
        sdr | '') ;;
        *) break ;;
    esac
done
front=$(luna-send -n 1 -f luna://com.webos.applicationManager/getForegroundAppInfo '{}' </dev/null 2>&1 |
    grep -o '"appId": *"[^"]*"' | sed 's/^[^:]*: *"//; s/"$//')
state playing

case $range in
    sdr | '') ;;
    *)
        echo "RESULT: HDR ($range) after $waited s"
        exit 0
        ;;
esac
if [ "$front" != "$player" ]; then
    echo "RESULT: the player is not in front (${front:-nothing}); see studio/tv/README.md"
    exit 2
fi
echo "RESULT: still SDR (dynamicRange=${range:-unknown}) after $waited s; see studio/tv/README.md"
exit 1
