#!/bin/bash
# usage: WYSS_AB_OUT=dir idle-ab.sh <binary> <label> <seconds>
# One run of a GUI binary on DISPLAY :77 (an isolated display the caller made), idle with the Console
# view shown. The PID is the one of the launch, checked against the binary's exact path before
# anything is read, and the only process ever ended. Never find a process by name: a copy of the
# binary has another name, and the owner's own app has the original (it happened, 2026-10-20).
set -u
BIN=$(readlink -f "$1"); LABEL=$2; SECS=$3
OUT=${WYSS_AB_OUT:?directory for the records}
mkdir -p $OUT
PIDFILE=$(mktemp)
( setsid env -u WAYLAND_DISPLAY DISPLAY=:77 GDK_BACKEND=x11 NO_AT_BRIDGE=1 "$BIN" > $OUT/$LABEL.app.log 2>&1 < /dev/null & echo $! > $PIDFILE )
sleep 3
APP=$(cat $PIDFILE)
if [ "$(readlink -f /proc/$APP/exe 2>/dev/null)" != "$BIN" ]; then
  echo "ABORT: pid $APP is not $BIN; nothing was ended" >&2; exit 1
fi
sleep 20
WEB=$(pgrep -P $APP -x WebKitWebProces | head -1)
if [ -z "$WEB" ]; then echo "ABORT: no web process under $APP" >&2; kill -TERM $APP; exit 1; fi
echo "label=$LABEL app=$APP web=$WEB loadavg=$(cut -d' ' -f1-3 /proc/loadavg)" | tee $OUT/$LABEL.txt
python3 -I $(dirname "$0")/threads.py $WEB $SECS > $OUT/$LABEL.web.txt &
python3 -I $(dirname "$0")/threads.py $APP $SECS > $OUT/$LABEL.app.txt
wait
tail -1 $OUT/$LABEL.web.txt | sed 's/^/web:  /' | tee -a $OUT/$LABEL.txt
tail -1 $OUT/$LABEL.app.txt | sed 's/^/app:  /' | tee -a $OUT/$LABEL.txt
if [ "$(readlink -f /proc/$APP/exe 2>/dev/null)" = "$BIN" ]; then kill -TERM $APP; fi
sleep 2
rm -f $PIDFILE
